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
    let assistant_test = args.iter().any(|value| value == "--assistant-test");
    let db = dir.join("demo.db");
    ai_terminal_server::account::manage_user(&db, "demo", &password, false)?;
    let mut router = ai_terminal_server::router(&db, &ai_terminal_security::random_secret()?)?;
    if assistant_test {
        router = router.route("/model/chat/completions", axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
            let task = body["messages"].as_array().and_then(|v| v.last()).and_then(|v| v["content"].as_str()).unwrap_or_default();
            let command = if task.contains("ls /Volumes/Code") {
                Some("ls /Volumes/Code")
            } else if task.contains("AI_DEVICE_OK") {
                Some("printf 'AI_DEVICE_OK\\n'; sleep 2; printf 'AI_DEVICE_DONE\\n'")
            } else { None };
            let message = if let Some(command) = command.filter(|_| body.get("tools").is_some()) {
                serde_json::json!({"role":"assistant","content":"Deterministic test model: requested terminal input.","tool_calls":[{"id":"fixture-input","type":"function","function":{"name":"terminal_input","arguments":serde_json::json!({"text":command,"submit":true}).to_string()}}]})
            } else {
                let observing = task.contains("Terminal snapshot revision");
                serde_json::json!({"role":"assistant","content":if observing { "Deterministic test model: terminal changes observed; this does not establish command success." } else { "Deterministic test model: no terminal input requested." }})
            };
            axum::Json(serde_json::json!({"choices":[{"message":message}]}))
        }));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut command = Command::new(cli);
    if assistant_test {
        command
            .env("AI_TERMINAL_AI_BASE_URL", format!("{url}/model"))
            .env("AI_TERMINAL_AI_MODEL", "deterministic-device-test")
            .env_remove("AI_TERMINAL_AI_API_KEY");
    }
    let mut child = command
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
            cols: 120,
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
    if args.iter().any(|value| value == "--bench") {
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
        &serde_json::json!({"server":url,"username":"demo","password":password,"session":info.id,"benchmarks":benchmarks,"assistant_test":assistant_test}),
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
