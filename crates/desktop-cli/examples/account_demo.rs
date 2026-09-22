//! Ephemeral account + Desktop fixture for native-app integration; secrets go to a private file.
use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request};
use ai_terminal_security::account::DesktopAccountCommand;
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let dir = PathBuf::from(args.get(1).context("account_demo STATE CLI")?);
    let cli = PathBuf::from(args.get(2).context("CLI required")?);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    let password = ai_terminal_security::random_secret()?;
    let db = dir.join("demo.db");
    ai_terminal_server::account::manage_user(&db, "demo", &password, false)?;
    let router = ai_terminal_server::router(&db, &ai_terminal_security::random_secret()?)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut child = Command::new(cli)
        .env("AI_TERMINAL_CREDENTIAL_STORE", "file")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .args(["--agent", "--state-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let until = Instant::now() + Duration::from_secs(5);
    let local = loop {
        if let Ok(c) = Client::connect(&dir) {
            break c;
        }
        anyhow::ensure!(Instant::now() < until, "agent startup failed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    local.call(Request {
        operation: Operation::Account as i32,
        text: serde_json::to_string(&DesktopAccountCommand::Login {
            server: url.clone(),
            ca: None,
            username: "demo".into(),
            password: password.clone(),
            device_name: "Local Desktop".into(),
        })?,
        ..Request::default()
    })?;
    let info = local
        .call(Request {
            operation: Operation::Create as i32,
            command: if cfg!(unix) {
                vec!["/bin/sh".into(), "-i".into()]
            } else {
                Vec::new()
            },
            rows: 24,
            cols: 80,
            ..Request::default()
        })?
        .info
        .context("missing session")?;
    local.call(Request {
        operation: Operation::Detach as i32,
        session: info.id.clone(),
        ..Request::default()
    })?;
    let mut benchmarks = Vec::new();
    if args.get(3).is_some_and(|value| value == "--bench") {
        for (name, rows, cols, output) in [
            ("idle-80x24", 24, 80, false),
            ("output-120x40", 40, 120, true),
            ("history-120x40", 40, 120, false),
        ] {
            let cwd = std::env::current_dir()?.join(&dir).join(name);
            std::fs::create_dir_all(&cwd)?;
            let code = r#"import os,tty,threading,time
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
            .replace("ENABLED", if output { "True" } else { "False" });
            let session = local
                .call(Request {
                    operation: Operation::Create as i32,
                    command: vec!["python3".into(), "-u".into(), "-c".into(), code],
                    cwd: cwd.to_string_lossy().into(),
                    rows,
                    cols,
                    ..Request::default()
                })?
                .info
                .context("missing benchmark session")?;
            local.call(Request {
                operation: Operation::Detach as i32,
                session: session.id.clone(),
                ..Request::default()
            })?;
            benchmarks
                .push(serde_json::json!({"id":session.id,"name":name,"rows":rows,"cols":cols}));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(dir.join("account-fixture.json"))?;
    serde_json::to_writer(
        file,
        &serde_json::json!({"server":url,"username":"demo","password":password,"session":info.id,"benchmarks":benchmarks}),
    )?;
    println!("Account fixture ready; credentials written to private fixture file");
    tokio::signal::ctrl_c().await?;
    let _ = local.call(Request {
        operation: Operation::Shutdown as i32,
        ..Request::default()
    });
    let _ = child.wait();
    server.abort();
    Ok(())
}
