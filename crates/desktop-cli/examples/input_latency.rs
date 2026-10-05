//! Isolated input-to-replica probe. A TCP delay proxy models ordered relay stalls, not UDP packet loss.
use ai_terminal_agent::Client;
use ai_terminal_mobile::RemoteTerminal;
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Result, ensure};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("relay");
    let rtt = args.get(2).map_or(Ok(0), |s| s.parse::<u64>())?;
    let stalls = args.get(3).is_some_and(|s| s == "stalls");
    let count = args.get(4).map_or(Ok(1000), |s| s.parse::<usize>())?;
    ensure!((1..=1600).contains(&count), "samples must be 1..=1600");
    let rows = args.get(5).map_or(Ok(24), |s| s.parse::<u32>())?;
    let cols = args.get(6).map_or(Ok(80), |s| s.parse::<u32>())?;
    let workload = args.get(7).cloned().unwrap_or_else(|| "idle".into());
    ensure!(
        matches!(workload.as_str(), "idle" | "output" | "history"),
        "unknown workload"
    );
    let dir = std::env::temp_dir().join(format!(
        "aiterminal-latency-{}",
        ai_terminal_agent::random_id()
    ));
    #[cfg(unix)]
    let mut directory = std::fs::DirBuilder::new();
    #[cfg(not(unix))]
    let directory = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(&dir)?;
    let executable = std::env::var_os("AI_TERMINAL_BENCH_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/debug/aTerminal"));
    let mut child = Command::new(executable)
        .env("AI_TERMINAL_CREDENTIAL_STORE", "file")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .args(["--agent", "--state-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let result = probe(&dir, mode, rtt, stalls, count, rows, cols, &workload).await;
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(dir);
    result
}
#[allow(clippy::too_many_arguments)]
async fn probe(
    dir: &std::path::Path,
    mode: &str,
    rtt: u64,
    stalls: bool,
    count: usize,
    rows: u32,
    cols: u32,
    workload: &str,
) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(10);
    let local = loop {
        if let Ok(c) = Client::connect(dir) {
            break c;
        }
        ensure!(Instant::now() < until, "agent startup");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let admin = ai_terminal_security::random_secret()?;
    let router = ai_terminal_server::router(&dir.join("server.db"), &admin)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", proxy.local_addr()?);
    let proxy_task = tokio::spawn(async move {
        while let Ok((a, _)) = proxy.accept().await {
            tokio::spawn(async move {
                if let Ok(b) = tokio::net::TcpStream::connect(address).await {
                    let _ = a.set_nodelay(true);
                    let _ = b.set_nodelay(true);
                    let (ar, aw) = a.into_split();
                    let (br, bw) = b.into_split();
                    tokio::join!(
                        delay(ar, bw, rtt / 4, stalls),
                        delay(br, aw, rtt / 4, stalls)
                    );
                }
            });
        }
    });
    let (pair, invite) = ai_terminal_remote::create_pair(&url, &admin, false).await?;
    std::fs::create_dir_all(dir.join("pairs"))?;
    std::fs::write(
        dir.join("pairs").join(format!("{}.json", pair.room)),
        serde_json::to_vec(&pair)?,
    )?;
    let command = if workload == "idle" {
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "stty -echo -icanon min 1 time 0; exec cat".into(),
        ]
    } else {
        vec![
            "python3".into(),
            "-u".into(),
            "-c".into(),
            r#"import os,tty,threading,time
 tty.setraw(0)
 lock=threading.Lock()
 os.write(1,b'history line\r\n'*200+b'\x1b[2J\x1b[H')
 def output():
  i=0
  while True:
   with lock: os.write(1,('\x1b7\x1b[ROW;1Hload:%08d\x1b8'%i).encode())
   i+=1
   time.sleep(1/30)
 if ENABLED: threading.Thread(target=output,daemon=True).start()
 while True:
  data=os.read(0,1024)
  if not data: break
  with lock: os.write(1,data)
"#
            .replace("\n ", "\n")
            .replace("ROW", &rows.to_string())
            .replace(
                "ENABLED",
                if workload == "output" {
                    "True"
                } else {
                    "False"
                },
            ),
        ]
    };
    let id = local
        .call(Request {
            operation: Operation::Create as i32,
            command,
            rows,
            cols,
            ..Request::default()
        })?
        .info
        .unwrap()
        .id;
    let invitation = invite.export()?;
    let direct = mode == "direct";
    let read_history = workload == "history";
    let (mut samples, elapsed) = tokio::task::spawn_blocking(move || -> Result<(Vec<f64>, f64)> {
        let mobile = std::sync::Arc::new(RemoteTerminal::new());
        mobile.connect(invitation)?;
        if !direct {
            mobile.use_relay()?
        }
        let mut frame = mobile.select(id, true)?;
        if direct {
            let until = Instant::now() + Duration::from_secs(8);
            while mobile.connection_path() != "direct" {
                mobile.refresh()?;
                ensure!(Instant::now() < until, "direct path not established");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        let history_core = mobile.clone();
        let history = if read_history {
            Some(std::thread::spawn(move || {
                while history_core.read_history().is_ok() {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }))
        } else {
            None
        };
        let start = Instant::now();
        let mut sent = Vec::new();
        let mut samples = Vec::new();
        let mut observed = 0;
        let mut next = start;
        while observed < count {
            if sent.len() < count && Instant::now() >= next {
                let event = Instant::now();
                match mobile.send_text("x".into(), false) {
                    Ok(()) => {
                        sent.push(event);
                        next = Instant::now() + Duration::from_millis(10);
                    }
                    Err(e) if e.to_string().contains("queue full") => {}
                    Err(e) => return Err(e.into()),
                }
            }
            if let Some(update) = mobile.refresh()? {
                frame = update;
            }
            let shown = frame.cells.iter().filter(|c| c.text == "x").count();
            ensure!(shown <= sent.len(), "duplicate echo");
            let now = Instant::now();
            for time in sent.iter().take(shown).skip(observed) {
                samples.push(now.duration_since(*time).as_secs_f64() * 1000.0);
            }
            observed = shown;
            ensure!(
                start.elapsed() < Duration::from_secs(900),
                "benchmark timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        mobile.disconnect()?;
        if let Some(history) = history {
            let _ = history.join();
        }
        Ok((samples, start.elapsed().as_secs_f64()))
    })
    .await??;
    samples.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"path":mode,"rows":rows,"cols":cols,"workload":workload,"injected_rtt_ms":rtt,"ordered_stalls_1pct":stalls,"samples":samples.len(),"elapsed_s":elapsed,"p50_ms":samples[samples.len()/2],"p95_ms":samples[(samples.len()*95).div_ceil(100)-1],"p99_ms":samples[(samples.len()*99).div_ceil(100)-1]})
    );
    local.call(Request {
        operation: Operation::Shutdown as i32,
        ..Request::default()
    })?;
    server.abort();
    proxy_task.abort();
    Ok(())
}
async fn delay(
    mut input: tokio::net::tcp::OwnedReadHalf,
    mut output: tokio::net::tcp::OwnedWriteHalf,
    ms: u64,
    stalls: bool,
) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(tokio::time::Instant, Vec<u8>)>(128);
    let reader = tokio::spawn(async move {
        let mut bytes = [0u8; 32768];
        let mut count = 0u64;
        while let Ok(n) = input.read(&mut bytes).await {
            if n == 0 {
                break;
            }
            count += 1;
            let stall = if stalls && count.is_multiple_of(100) {
                150
            } else {
                0
            };
            if tx
                .send((
                    tokio::time::Instant::now() + Duration::from_millis(ms + stall),
                    bytes[..n].to_vec(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    while let Some((at, bytes)) = rx.recv().await {
        tokio::time::sleep_until(at).await;
        if output.write_all(&bytes).await.is_err() {
            break;
        }
    }
    reader.abort();
}
