//! Real CLI/local RPC, isolated daemon, no model provider or existing user state.
use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Desktop {
    child: Child,
    dir: PathBuf,
}
impl Desktop {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aterminal-authorization-cli-{}",
            ai_terminal_agent_runtime::request_id()
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_aTerminal"))
            .args(["--agent", "--state-dir"])
            .arg(&dir)
            .env("AI_TERMINAL_CREDENTIAL_STORE", "file")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let desktop = Self { child, dir };
        let deadline = Instant::now() + Duration::from_secs(8);
        while Client::connect(&desktop.dir).is_err() {
            assert!(
                Instant::now() < deadline,
                "isolated daemon startup timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        desktop
    }
    fn cli(&self, args: &[&str]) -> (bool, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_aTerminal"))
            .arg("--state-dir")
            .arg(&self.dir)
            .arg("--json")
            .arg("agents")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "invalid CLI JSON: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), value)
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        if let Ok(client) = Client::connect(&self.dir) {
            let _ = client.call(Request {
                operation: Operation::Shutdown as i32,
                ..Default::default()
            });
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
#[test]
fn non_tty_json_cli_uses_revisioned_user_rpc_and_never_prompts_or_silently_authorizes() {
    let desktop = Desktop::start();
    let (ok, initial) = desktop.cli(&["permissions"]);
    assert!(ok);
    assert_eq!(initial["result"]["full_authorization"], false);
    assert_eq!(initial["result"]["permission_mode"], "ask");
    let (ok, changed) = desktop.cli(&[
        "permissions",
        "--expected-revision",
        "0",
        "--full-authorization",
        "true",
    ]);
    assert!(ok);
    assert_eq!(changed["result"]["revision"], 1);
    let (ok, conflict) = desktop.cli(&[
        "permissions",
        "--expected-revision",
        "0",
        "--full-authorization",
        "false",
    ]);
    assert!(!ok);
    assert!(
        conflict
            .to_string()
            .contains("permission_revision_conflict")
    );
    let (ok, state) = desktop.cli(&["show"]);
    assert!(ok);
    assert_eq!(state["result"]["permissions"]["full_authorization"], true);
    assert_eq!(state["result"]["pending"]["items"], json!([]));
    let (ok, readonly) = desktop.cli(&[
        "permissions",
        "--expected-revision",
        "1",
        "--permission-mode",
        "read_only",
    ]);
    assert!(ok);
    assert_eq!(readonly["result"]["full_authorization"], false);
    for command in [vec!["pending"], vec!["rules"]] {
        let (ok, value) = desktop.cli(&command);
        assert!(ok);
        assert_eq!(value["result"]["items"], json!([]));
    }
    let (ok, missing) = desktop.cli(&["resolve", "unknown", "--decision", "once"]);
    assert!(!ok);
    assert!(missing.to_string().contains("pending_not_found"));
    let (ok, send) = desktop.cli(&["send", "--message", "never call a paid model"]);
    assert!(!ok);
    assert!(send.to_string().contains("model_not_configured"));
    let (ok, still) = desktop.cli(&["permissions"]);
    assert!(ok);
    assert_eq!(still["result"]["full_authorization"], false);
    assert_eq!(still["result"]["revision"], 2);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_pending_once_resolution_crosses_real_host_and_pty_without_replay() {
    use ai_terminal_agent_runtime::config::{
        Binding, Capabilities, ModelProfile, OwnerConfig, Provider, Reasoning,
    };
    use ai_terminal_agent_runtime::model::{Connection, Protocol};
    use axum::response::IntoResponse;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };
    let desktop = Desktop::start();
    let marker = desktop.dir.join("once-result");
    let command = format!("/usr/bin/printf CLI >> '{}'", marker.to_string_lossy());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicU32::new(0));
    let counter = calls.clone();
    let tool_command = command.clone();
    let router=axum::Router::new().route("/chat/completions",axum::routing::post(move || {
        let counter=counter.clone();let command=tool_command.clone();async move {
            let first=counter.fetch_add(1,Ordering::AcqRel)==0;
            let delta=if first {json!({"tool_calls":[{"index":0,"id":"cli-command","type":"function","function":{"name":"run_command","arguments":json!({"command":command}).to_string()}}]})} else {json!({"content":"Complete"})};
            let a=json!({"id":"local","object":"chat.completion.chunk","created":0,"model":"local","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let b=json!({"id":"local","object":"chat.completion.chunk","created":0,"model":"local","choices":[{"index":0,"delta":{},"finish_reason":if first {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
            ([("content-type","text/event-stream")],format!("data: {a}\n\ndata: {b}\n\ndata: [DONE]\n\n")).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = Client::connect(&desktop.dir).unwrap();
    let provider = Provider {
        id: "local".into(),
        name: "local stub".into(),
        connection: Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: format!("http://{address}"),
            api_version: None,
        },
        catalog_url: None,
        secret_ref: None,
        credential_revision: 0,
        enabled: true,
    };
    let profile = ModelProfile {
        id: "local".into(),
        name: "local stub".into(),
        provider_id: "local".into(),
        model: "local".into(),
        context_window: 128000,
        max_tokens: 2048,
        temperature: None,
        top_p: None,
        reasoning: Reasoning::ProviderDefault,
        capabilities: Capabilities {
            tools: Some(true),
            streaming: Some(true),
            ..Default::default()
        },
        max_rounds: 4,
        max_seconds: 15,
        read_only: false,
    };
    let mut config = OwnerConfig::default();
    config.providers.insert("local".into(), provider);
    config.models.insert("local".into(), profile);
    config.bindings.insert(
        "session-default".into(),
        Binding {
            model_id: "local".into(),
            reasoning: None,
        },
    );
    client
        .call(Request {
            operation: Operation::Configuration as i32,
            text: json!({"action":"replace","expected_revision":0,"config":config,"secrets":{}})
                .to_string(),
            ..Default::default()
        })
        .unwrap();
    // This synthetic shell has no startup files or hooks. It executes only this test's
    // explicitly approved literal command and never touches existing terminals.
    let terminal = client
        .call(Request {
            operation: Operation::Create as i32,
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "while IFS= read -r line; do eval \"$line\"; done".into(),
            ],
            cwd: desktop.dir.to_string_lossy().into_owned(),
            rows: 24,
            cols: 80,
            ..Default::default()
        })
        .unwrap()
        .info
        .unwrap();
    client
        .call(Request {
            operation: Operation::AttachDesktop as i32,
            session: terminal.id.clone(),
            ..Default::default()
        })
        .unwrap();
    let (ok, _) = desktop.cli(&[
        "send",
        "--session",
        &terminal.id,
        "--message",
        "perform isolated operation",
        "--request-id",
        "root",
    ]);
    assert!(ok);
    let deadline = Instant::now() + Duration::from_secs(8);
    let pending = loop {
        let (ok, page) = desktop.cli(&["pending", "--session", &terminal.id]);
        assert!(ok);
        if let Some(item) = page["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["state"] == "pending")
        {
            break item.clone();
        }
        assert!(Instant::now() < deadline, "pending did not appear");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(!marker.exists());
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(pending["kind"], "approval");
    assert!(
        pending["arguments_preview"]
            .as_str()
            .unwrap()
            .contains("once-result")
    );
    let id = pending["id"].as_str().unwrap();
    let (ok, result) = desktop.cli(&[
        "resolve",
        id,
        "--session",
        &terminal.id,
        "--decision",
        "once",
        "--request-id",
        "once",
    ]);
    assert!(ok);
    assert_eq!(result["result"]["duplicate"], false);
    let deadline = Instant::now() + Duration::from_secs(8);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "approved PTY action did not execute"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (ok, result) = desktop.cli(&[
        "resolve",
        id,
        "--session",
        &terminal.id,
        "--decision",
        "once",
        "--request-id",
        "once",
    ]);
    assert!(ok);
    assert_eq!(result["result"]["duplicate"], true);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "CLI");
    let (ok, permissions) = desktop.cli(&["permissions", "--session", &terminal.id]);
    assert!(ok);
    assert_eq!(permissions["result"]["full_authorization"], false);
    client
        .call(Request {
            operation: Operation::Close as i32,
            session: terminal.id,
            ..Default::default()
        })
        .unwrap();
    server.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_native_program_always_appends_exact_bytes_then_revokes_denies_and_regrants() {
    use ai_terminal_agent_runtime::config::{
        Binding, Capabilities, ModelProfile, OwnerConfig, Provider, Reasoning,
    };
    use ai_terminal_agent_runtime::model::{Connection, Protocol};
    use axum::response::IntoResponse;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };
    let desktop = Desktop::start();
    let marker = desktop.dir.join("once-result");
    let command =
        json!({"program":"/usr/bin/tee","args":["-a",marker.to_string_lossy()],"stdin":"CLI"});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicU32::new(0));
    let counter = calls.clone();
    let tool_command = command.clone();
    let router=axum::Router::new().route("/chat/completions",axum::routing::post(move || {
        let counter=counter.clone();let command=tool_command.clone();async move {
            let phase=counter.fetch_add(1,Ordering::AcqRel);let first=phase==0;
            let delta=if first {json!({"tool_calls":[{"index":0,"id":"cli-command","type":"function","function":{"name":"run_program","arguments":command.to_string()}}]})} else if phase==1 {json!({"content":json!({"summary":"Native child output observed","tui_lines":[]}).to_string()})} else {json!({"content":"Complete"})};
            let a=json!({"id":"local","object":"chat.completion.chunk","created":0,"model":"local","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let b=json!({"id":"local","object":"chat.completion.chunk","created":0,"model":"local","choices":[{"index":0,"delta":{},"finish_reason":if first {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
            ([("content-type","text/event-stream")],format!("data: {a}\n\ndata: {b}\n\ndata: [DONE]\n\n")).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = Client::connect(&desktop.dir).unwrap();
    let provider = Provider {
        id: "local".into(),
        name: "local stub".into(),
        connection: Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: format!("http://{address}"),
            api_version: None,
        },
        catalog_url: None,
        secret_ref: None,
        credential_revision: 0,
        enabled: true,
    };
    let profile = ModelProfile {
        id: "local".into(),
        name: "local stub".into(),
        provider_id: "local".into(),
        model: "local".into(),
        context_window: 128000,
        max_tokens: 2048,
        temperature: None,
        top_p: None,
        reasoning: Reasoning::ProviderDefault,
        capabilities: Capabilities {
            tools: Some(true),
            streaming: Some(true),
            ..Default::default()
        },
        max_rounds: 4,
        max_seconds: 15,
        read_only: false,
    };
    let mut config = OwnerConfig::default();
    config.providers.insert("local".into(), provider);
    config.models.insert("local".into(), profile);
    config.bindings.insert(
        "session-default".into(),
        Binding {
            model_id: "local".into(),
            reasoning: None,
        },
    );
    client
        .call(Request {
            operation: Operation::Configuration as i32,
            text: json!({"action":"replace","expected_revision":0,"config":config,"secrets":{}})
                .to_string(),
            ..Default::default()
        })
        .unwrap();
    // This synthetic shell has no startup files or hooks. It executes only this test's
    // explicitly approved literal command and never touches existing terminals.
    let terminal = client
        .call(Request {
            operation: Operation::Create as i32,
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "while IFS= read -r line; do eval \"$line\"; done".into(),
            ],
            cwd: desktop.dir.to_string_lossy().into_owned(),
            rows: 24,
            cols: 80,
            ..Default::default()
        })
        .unwrap()
        .info
        .unwrap();
    client
        .call(Request {
            operation: Operation::AttachDesktop as i32,
            session: terminal.id.clone(),
            ..Default::default()
        })
        .unwrap();
    let (ok, _) = desktop.cli(&[
        "send",
        "--session",
        &terminal.id,
        "--message",
        "perform isolated operation",
        "--request-id",
        "root",
    ]);
    assert!(ok);
    let deadline = Instant::now() + Duration::from_secs(8);
    let pending = loop {
        let (ok, page) = desktop.cli(&["pending", "--session", &terminal.id]);
        assert!(ok);
        if let Some(item) = page["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["state"] == "pending")
        {
            break item.clone();
        }
        assert!(Instant::now() < deadline, "pending did not appear");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(!marker.exists());
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(pending["kind"], "approval");
    assert_eq!(pending["can_always"], true, "{pending}");
    assert!(pending["fingerprint"].as_str().unwrap().starts_with("v3:"));
    assert!(
        pending["arguments_preview"]
            .as_str()
            .unwrap()
            .contains("once-result")
    );
    let id = pending["id"].as_str().unwrap();
    let (ok, result) = desktop.cli(&[
        "resolve",
        id,
        "--session",
        &terminal.id,
        "--decision",
        "always",
        "--request-id",
        "always1",
    ]);
    assert!(ok);
    assert_eq!(result["result"]["duplicate"], false);
    async fn completed(desktop: &Desktop, session: &str) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let (ok, state) = desktop.cli(&["show", "--session", session]);
            assert!(ok);
            if state["result"]["state"] == "completed" {
                break;
            }
            assert!(Instant::now() < deadline, "native Run failed: {state}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    completed(&desktop, &terminal.id).await;
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "CLI");
    let (ok, rules) = desktop.cli(&["rules"]);
    assert!(ok);
    let rule = rules["result"]["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    calls.store(0, Ordering::Release);
    assert!(
        desktop
            .cli(&[
                "send",
                "--session",
                &terminal.id,
                "--message",
                "same exact native operation",
                "--request-id",
                "root2"
            ])
            .0
    );
    completed(&desktop, &terminal.id).await;
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "CLICLI");
    assert!(
        desktop
            .cli(&["revoke-rule", &rule, "--request-id", "revoke1"])
            .0
    );
    calls.store(0, Ordering::Release);
    assert!(
        desktop
            .cli(&[
                "send",
                "--session",
                &terminal.id,
                "--message",
                "same exact native operation",
                "--request-id",
                "root3"
            ])
            .0
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    let denied = loop {
        let (ok, page) = desktop.cli(&["pending", "--session", &terminal.id]);
        assert!(ok);
        if let Some(row) = page["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["state"] == "pending")
        {
            break row.clone();
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(
        desktop
            .cli(&[
                "resolve",
                denied["id"].as_str().unwrap(),
                "--session",
                &terminal.id,
                "--decision",
                "deny",
                "--request-id",
                "deny"
            ])
            .0
    );
    completed(&desktop, &terminal.id).await;
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "CLICLI");
    calls.store(0, Ordering::Release);
    assert!(
        desktop
            .cli(&[
                "send",
                "--session",
                &terminal.id,
                "--message",
                "same exact native operation",
                "--request-id",
                "root4"
            ])
            .0
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    let regrant = loop {
        let (ok, page) = desktop.cli(&["pending", "--session", &terminal.id]);
        assert!(ok);
        if let Some(row) = page["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["state"] == "pending")
        {
            break row.clone();
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(
        desktop
            .cli(&[
                "resolve",
                regrant["id"].as_str().unwrap(),
                "--session",
                &terminal.id,
                "--decision",
                "always",
                "--request-id",
                "always2"
            ])
            .0
    );
    completed(&desktop, &terminal.id).await;
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "CLICLICLI");
    let (ok, rules) = desktop.cli(&["rules"]);
    assert!(ok);
    assert_eq!(rules["result"]["items"][0]["id"], rule);
    assert!(
        desktop
            .cli(&["revoke-rule", &rule, "--request-id", "revoke2"])
            .0
    );
    assert_eq!(desktop.cli(&["rules"]).1["result"]["items"], json!([]));
    client
        .call(Request {
            operation: Operation::Close as i32,
            session: terminal.id,
            ..Default::default()
        })
        .unwrap();
    server.abort();
}
