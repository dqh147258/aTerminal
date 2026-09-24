use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request, SESSION_CLOSED_ERROR};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Host {
    child: Child,
    dir: PathBuf,
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn host() -> (Host, Client) {
    host_with_model(None)
}
fn host_with_model(model_url: Option<&str>) -> (Host, Client) {
    let dir = std::env::temp_dir().join(format!(
        "ai-terminal-test-{:x}",
        ai_terminal_agent::random_id()
    ));
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai-terminal"));
    command
        .env_remove("AI_TERMINAL_AI_BASE_URL")
        .env_remove("AI_TERMINAL_AI_MODEL")
        .env_remove("AI_TERMINAL_AI_API_KEY");
    if let Some(url) = model_url {
        command
            .env("AI_TERMINAL_AI_BASE_URL", url)
            .env("AI_TERMINAL_AI_MODEL", "test");
    }
    let child = command
        .env("AI_TERMINAL_CREDENTIAL_STORE", "file")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .args(["--agent", "--state-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let host = Host { child, dir };
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(c) = Client::connect(&host.dir) {
            return (host, c);
        }
        assert!(Instant::now() < until, "Agent startup timed out");
        thread::sleep(Duration::from_millis(20));
    }
}
fn attach_desktop(client: &Client, id: &str) -> ai_terminal_protocol::local::SessionInfo {
    client
        .call(Request {
            operation: Operation::AttachDesktop as i32,
            session: id.into(),
            ..Request::default()
        })
        .unwrap()
        .info
        .unwrap()
}
#[test]
fn detach_retains_process_and_control_fences_duplicate_input() {
    let (mut host, client) = host();
    #[cfg(unix)]
    let command = vec!["/bin/sh".into(), "-i".into()];
    #[cfg(windows)]
    let command = vec![
        "powershell.exe".into(),
        "-NoProfile".into(),
        "-NoLogo".into(),
    ];
    let created = client
        .call(Request {
            operation: Operation::Create as i32,
            command,
            rows: 12,
            cols: 80,
            ..Request::default()
        })
        .unwrap_or_else(|e| {
            panic!(
                "create failed: {e}; Agent status: {:?}",
                host.child.try_wait()
            )
        });
    let info = created.info.unwrap();
    let id = info.id.clone();
    assert!(attach_desktop(&client, &id).desktop_attached);
    let input = Request {
        session: id.clone(),
        session_epoch: info.epoch,
        operation: Operation::Input as i32,
        control_epoch: info.control_epoch,
        input_seq: 1,
        input: b"echo FIRST\r".to_vec(),
        ..Request::default()
    };
    client.call(input.clone()).unwrap();
    assert_eq!(client.call(input.clone()).unwrap().accepted_input_seq, 1);
    let mut conflicting = input.clone();
    conflicting.input = b"echo WRONG\r".to_vec();
    assert!(client.call(conflicting).is_err());
    client
        .call(Request {
            session: id.clone(),
            operation: Operation::Detach as i32,
            ..Request::default()
        })
        .unwrap();
    assert!(
        !client
            .call(Request {
                session: id.clone(),
                operation: Operation::Poll as i32,
                ..Request::default()
            })
            .unwrap()
            .info
            .unwrap()
            .desktop_attached
    );
    assert!(client.call(input.clone()).is_err());
    let second = Client::connect(&host.dir).unwrap();
    let acquired = second
        .call(Request {
            session: id.clone(),
            operation: Operation::Acquire as i32,
            ..Request::default()
        })
        .unwrap()
        .info
        .unwrap();
    assert_eq!(acquired.epoch, info.epoch);
    assert!(!acquired.exited);
    assert!(!acquired.desktop_attached);
    let mut stale = input;
    stale.input_seq = 2;
    assert!(client.call(stale).is_err());
    assert!(
        second
            .call(Request {
                session: id.clone(),
                operation: Operation::Input as i32,
                control_epoch: acquired.control_epoch,
                input_seq: 1,
                input: b"not yet".to_vec(),
                ..Request::default()
            })
            .is_err()
    );
    attach_desktop(&second, &id);
    second
        .call(Request {
            session: id.clone(),
            operation: Operation::Input as i32,
            control_epoch: acquired.control_epoch,
            input_seq: 1,
            input: b"echo SECOND\r".to_vec(),
            ..Request::default()
        })
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let reply = second
            .call(Request {
                session: id.clone(),
                operation: Operation::Poll as i32,
                ..Request::default()
            })
            .unwrap();
        let frame = reply.snapshot.unwrap();
        if has_output(
            frame.cells.iter().map(|c| c.text.as_str()),
            frame.cols as usize,
            "SECOND",
        ) {
            break;
        }
        assert!(Instant::now() < until, "detached shell stopped");
        thread::sleep(Duration::from_millis(10));
    }
    second
        .call(Request {
            session: id.clone(),
            operation: Operation::Close as i32,
            ..Request::default()
        })
        .unwrap();
    assert_eq!(
        second
            .call(Request {
                session: id,
                operation: Operation::Poll as i32,
                ..Request::default()
            })
            .unwrap_err()
            .to_string(),
        SESSION_CLOSED_ERROR
    );
    assert!(second.call(Request::default()).unwrap().sessions.is_empty());
    second
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    assert!(host.child.wait().unwrap().success());
}

#[cfg(unix)]
#[test]
fn git_status_keeps_publishing_valid_intermediate_frames() {
    let (host, client) = host();
    let repo = host.dir.join("status-repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("tracked.txt"), "before\n").unwrap();
    git(&["add", "tracked.txt"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "initial",
    ]);
    std::fs::write(repo.join("tracked.txt"), "after\n").unwrap();
    let reply = client
        .call(Request {
            operation: Operation::Create as i32,
            cwd: repo.to_string_lossy().into_owned(),
            command: vec!["/bin/sh".into(), "-c".into(), "git status; sleep 1".into()],
            rows: 24,
            cols: 80,
            ..Request::default()
        })
        .unwrap();
    let info = reply.info.unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let reply = client
            .call(Request {
                operation: Operation::Poll as i32,
                session: info.id.clone(),
                ..Request::default()
            })
            .unwrap();
        let state = reply.info.unwrap();
        assert!(
            state.error.is_empty(),
            "Agent rejected git status frame: {}",
            state.error
        );
        let frame = reply.snapshot.unwrap();
        frame.validate().unwrap();
        let screen: String = frame.cells.iter().map(|c| c.text.as_str()).collect();
        if screen.contains("modified:   tracked.txt") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "git status output did not appear: {screen:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    client
        .call(Request {
            operation: Operation::Close as i32,
            session: info.id,
            ..Request::default()
        })
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encrypted_relay_reaches_the_same_pty_and_revocation_closes_it() {
    let (mut host, local) = host();
    let admin = ai_terminal_security::random_secret().unwrap();
    let router = ai_terminal_server::router(&host.dir.join("relay.sqlite3"), &admin).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let (pair, invite) = ai_terminal_remote::create_pair(&url, &admin, false)
        .await
        .unwrap();
    std::fs::create_dir_all(host.dir.join("pairs")).unwrap();
    std::fs::write(
        host.dir.join("pairs").join(format!("{}.json", pair.room)),
        serde_json::to_vec(&pair).unwrap(),
    )
    .unwrap();
    let mut remote = ai_terminal_remote::Channel::connect(&invite).await.unwrap();
    #[cfg(unix)]
    let command = vec!["/bin/sh".into(), "-i".into()];
    #[cfg(windows)]
    let command = vec![
        "powershell.exe".into(),
        "-NoProfile".into(),
        "-NoLogo".into(),
    ];
    let created = remote
        .request(Request {
            operation: Operation::Create as i32,
            command,
            rows: 12,
            cols: 80,
            ..Request::default()
        })
        .await
        .unwrap();
    assert!(created.error.is_empty(), "{}", created.error);
    let info = created.info.unwrap();
    assert!(!info.desktop_attached);
    attach_desktop(&local, &info.id);
    remote
        .request(Request {
            session: info.id.clone(),
            session_epoch: info.epoch,
            operation: Operation::Input as i32,
            control_epoch: info.control_epoch,
            input_seq: 1,
            input: b"echo E2E_RELAY_OK\r".to_vec(),
            ..Request::default()
        })
        .await
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let reply = local
            .call(Request {
                session: info.id.clone(),
                operation: Operation::Poll as i32,
                ..Request::default()
            })
            .unwrap();
        let frame = reply.snapshot.unwrap();
        if has_output(
            frame.cells.iter().map(|c| c.text.as_str()),
            frame.cols as usize,
            "E2E_RELAY_OK",
        ) {
            break;
        }
        assert!(Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let response = reqwest::Client::new()
        .delete(format!("{url}/v1/pairs/{}", pair.room))
        .bearer_auth(admin)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 204);
    assert!(remote.request(Request::default()).await.is_err());
    assert!(!local.call(Request::default()).unwrap().sessions[0].exited);
    local
        .call(Request {
            session: info.id,
            operation: Operation::Close as i32,
            ..Request::default()
        })
        .unwrap();
    local
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_core_reads_types_and_cannot_override_readonly_pair() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (mut host, local) = host_with_model(Some(&format!("{url}/model")));
    let admin = ai_terminal_security::random_secret().unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let model_calls = calls.clone();
    let router = ai_terminal_server::router(&host.dir.join("mobile.db"), &admin).unwrap()
        .route("/model/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let calls = model_calls.clone();
            async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert_eq!(body["model"], "test");
                assert_eq!(body["messages"].as_array().unwrap().len(), 2);
                tokio::time::sleep(Duration::from_millis(300)).await;
                axum::Json(serde_json::json!({"choices":[{"message":{"content":"fixture explanation, not executed"}}]}))
            }
        }));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let (pair, invite) = ai_terminal_remote::create_pair(&url, &admin, false)
        .await
        .unwrap();
    std::fs::create_dir_all(host.dir.join("pairs")).unwrap();
    std::fs::write(
        host.dir.join("pairs").join(format!("{}.json", pair.room)),
        serde_json::to_vec(&pair).unwrap(),
    )
    .unwrap();
    let invitation = invite.export().unwrap();
    let desktop = local.clone();
    let id = tokio::task::spawn_blocking(move || {
        let mobile = ai_terminal_mobile::RemoteTerminal::new();
        mobile.connect(invitation).unwrap();
        let session = mobile.create_session(String::new()).unwrap();
        attach_desktop(&desktop, &session.id);
        mobile.select(session.id.clone(), true).unwrap();
        let request = r#"{"action":"send","request_id":"round-1","message":"explain"}"#.to_owned();
        let status: serde_json::Value = serde_json::from_str(
            &mobile
                .assistant(session.id.clone(), r#"{"action":"status"}"#.into())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(status["available"], true);
        let started = Instant::now();
        mobile
            .assistant(session.id.clone(), request.clone())
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        mobile.assistant(session.id.clone(), request).unwrap();
        mobile
            .send_text("echo MOBILE_CORE_OK".into(), true)
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(frame) = mobile.refresh().unwrap()
                && has_output(
                    frame.cells.iter().map(|c| c.text.as_str()),
                    frame.cols as usize,
                    "MOBILE_CORE_OK",
                )
            {
                break;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(30));
        }
        desktop
            .call(Request {
                operation: Operation::Detach as i32,
                session: session.id.clone(),
                ..Request::default()
            })
            .unwrap();
        let until_read_only = Instant::now() + Duration::from_secs(5);
        while mobile.has_control() && Instant::now() < until_read_only {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !mobile.has_control(),
            "Desktop detach did not make Mobile read-only"
        );
        assert!(!mobile.desktop_attached());
        assert!(mobile.send_text("must not run".into(), true).is_err());
        mobile.read_history().unwrap();
        attach_desktop(&desktop, &session.id);
        let until_resume = Instant::now() + Duration::from_secs(5);
        while !mobile.has_control() && Instant::now() < until_resume {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            mobile.has_control(),
            "Desktop attach did not restore Mobile input"
        );
        mobile
            .send_text("echo MOBILE_RESUMED".into(), true)
            .unwrap();
        let until_output = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(frame) = mobile.refresh().unwrap()
                && has_output(
                    frame.cells.iter().map(|c| c.text.as_str()),
                    frame.cols as usize,
                    "MOBILE_RESUMED",
                )
            {
                break;
            }
            assert!(Instant::now() < until_output, "resumed output missing");
            thread::sleep(Duration::from_millis(30));
        }
        loop {
            let response: serde_json::Value = serde_json::from_str(
                &mobile
                    .assistant(
                        session.id.clone(),
                        r#"{"action":"poll","request_id":"round-1"}"#.into(),
                    )
                    .unwrap(),
            )
            .unwrap();
            if response["state"] == "completed" {
                assert_eq!(response["reply"], "fixture explanation, not executed");
                break;
            }
            assert!(
                Instant::now() < until,
                "assistant request did not finish: {response}"
            );
            thread::sleep(Duration::from_millis(30));
        }
        assert!(
            mobile
                .assistant("missing-session".into(), r#"{"action":"status"}"#.into())
                .is_err()
        );
        mobile.send_text("exit".into(), true).unwrap();
        let until_exit = Instant::now() + Duration::from_secs(5);
        loop {
            if desktop
                .call(Request {
                    operation: Operation::Poll as i32,
                    session: session.id.clone(),
                    ..Request::default()
                })
                .unwrap()
                .info
                .unwrap()
                .exited
            {
                break;
            }
            assert!(Instant::now() < until_exit, "Shell did not exit");
            thread::sleep(Duration::from_millis(20));
        }
        let final_screen = mobile.select(session.id.clone(), true).unwrap();
        assert!(mobile.session_exited());
        assert!(!mobile.has_control());
        assert!(has_output(
            final_screen.cells.iter().map(|cell| cell.text.as_str()),
            final_screen.cols as usize,
            "MOBILE_RESUMED"
        ));
        mobile.read_history().unwrap();
        assert!(mobile.send_text("cannot execute".into(), true).is_err());
        mobile.disconnect().unwrap();
        session.id
    })
    .await
    .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (pair, invite) = ai_terminal_remote::create_pair(&url, &admin, true)
        .await
        .unwrap();
    std::fs::write(
        host.dir.join("pairs").join(format!("{}.json", pair.room)),
        serde_json::to_vec(&pair).unwrap(),
    )
    .unwrap();
    let invitation = invite.export().unwrap();
    let selected = id.clone();
    tokio::task::spawn_blocking(move || {
        let mobile = ai_terminal_mobile::RemoteTerminal::new();
        mobile.connect(invitation).unwrap();
        mobile.select(selected.clone(), true).unwrap();
        assert!(
            mobile
                .assistant(
                    selected,
                    r#"{"action":"send","request_id":"read-only","message":"explain"}"#.into()
                )
                .is_err()
        );
        assert!(!mobile.has_control());
        assert!(mobile.send_text("must not run".into(), true).is_err());
        mobile.disconnect().unwrap();
    })
    .await
    .unwrap();
    local
        .call(Request {
            session: id,
            operation: Operation::Close as i32,
            ..Request::default()
        })
        .unwrap();
    local
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}

fn has_output<'a>(cells: impl Iterator<Item = &'a str>, cols: usize, expected: &str) -> bool {
    // Do not confuse the terminal echo of an entered command with its actual output.
    cells
        .collect::<Vec<_>>()
        .chunks(cols)
        .any(|line| line.concat().trim() == expected)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assistant_inputs_once_monitors_real_pty_and_fences_stale_writes() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (mut host, local) = host_with_model(Some(&format!("{url}/model")));
    let admin = ai_terminal_security::random_secret().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let model_calls = calls.clone();
    let router = ai_terminal_server::router(&host.dir.join("assistant.db"), &admin).unwrap()
        .route("/model/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let calls = model_calls.clone();
            async move {
                if body.get("tools").is_some() {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(350)).await;
                    let task = body["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap();
                    let text = match task {
                        "execute" => "printf 'AI_FENCED_ONCE\\n'; sleep 1; printf 'AI_FENCED_DONE\\n'",
                        "stale" => "printf 'MUST_NOT_RUN\\n'",
                        "lost-control" => "printf 'LOST_MUST_NOT_RUN\\n'",
                        "cancel-before" => "printf 'CANCEL_MUST_NOT_RUN\\n'",
                        _ => panic!("unexpected fixture task"),
                    };
                    axum::Json(serde_json::json!({"choices":[{"message":{"content":null,"tool_calls":[{"type":"function","function":{"name":"terminal_input","arguments":serde_json::json!({"text":text,"submit":true}).to_string()}}]}}]}))
                } else {
                    assert!(body["messages"][1]["content"].as_str().unwrap().contains("Terminal snapshot revision"));
                    axum::Json(serde_json::json!({"choices":[{"message":{"content":"Observed a real terminal revision; execution status is inferred."}}]}))
                }
            }
        }));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let (pair, invite) = ai_terminal_remote::create_pair(&url, &admin, false)
        .await
        .unwrap();
    std::fs::create_dir_all(host.dir.join("pairs")).unwrap();
    std::fs::write(
        host.dir.join("pairs").join(format!("{}.json", pair.room)),
        serde_json::to_vec(&pair).unwrap(),
    )
    .unwrap();
    let desktop = local.clone();
    tokio::task::spawn_blocking(move || {
        let mobile = ai_terminal_mobile::RemoteTerminal::new();
        mobile.connect(invite.export().unwrap()).unwrap();
        let session = mobile.create_session(String::new()).unwrap();
        attach_desktop(&desktop, &session.id);
        mobile.select(session.id.clone(), false).unwrap();
        let send = |id: &str, task: &str| serde_json::json!({"action":"send","request_id":id,"message":task,"allow_input":true,"monitor":true,"include_screen":true}).to_string();
        let call = |request: String| -> serde_json::Value {
            serde_json::from_str(&mobile.assistant(session.id.clone(), request).unwrap()).unwrap()
        };
        let poll = |id: &str, state: &str| -> serde_json::Value {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                let response = call(serde_json::json!({"action":"poll","request_id":id}).to_string());
                if response["state"] == state { return response; }
                assert!(Instant::now() < until, "assistant state did not become {state}: {response}");
                thread::sleep(Duration::from_millis(30));
            }
        };
        let assert_output = |expected: &str| {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(frame) = mobile.refresh().unwrap()
                    && has_output(frame.cells.iter().map(|c| c.text.as_str()), frame.cols as usize, expected) { break; }
                assert!(Instant::now() < until, "missing terminal output {expected}");
                thread::sleep(Duration::from_millis(30));
            }
        };
        call(send("once", "execute"));
        call(send("once", "execute"));
        assert!(mobile.has_control());
        assert_output("AI_FENCED_DONE");
        let until = Instant::now() + Duration::from_secs(8);
        loop {
            let response = poll("once", "monitoring");
            if response["events"].as_array().unwrap().iter().any(|e| e["kind"] == "observation") { break; }
            assert!(Instant::now() < until, "missing monitoring observation");
            thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        mobile.send_text("printf 'AFTER_AI\\n'".into(), true).unwrap();
        assert_output("AFTER_AI");
        call(serde_json::json!({"action":"cancel","request_id":"once"}).to_string());
        assert_eq!(poll("once", "stopped")["monitoring"], false);

        call(send("stale", "stale"));
        mobile.send_text("printf 'MANUAL_WINS\\n'".into(), true).unwrap();
        let failure = poll("stale", "failed");
        assert!(failure["message"].as_str().unwrap().contains("manual input"));
        assert_output("MANUAL_WINS");
        call(send("lost", "lost-control"));
        mobile.select(session.id.clone(), false).unwrap();
        let message = poll("lost", "failed")["message"].as_str().unwrap().to_owned();
        assert!(message.contains("input stream expired") || message.contains("terminal control changed"), "{message}");
        call(send("cancel", "cancel-before"));
        call(serde_json::json!({"action":"cancel","request_id":"cancel"}).to_string());
        poll("cancel", "stopped");
        let frame = mobile.refresh().unwrap().unwrap();
        for forbidden in ["MUST_NOT_RUN", "LOST_MUST_NOT_RUN", "CANCEL_MUST_NOT_RUN"] {
            assert!(!has_output(frame.cells.iter().map(|c| c.text.as_str()), frame.cols as usize, forbidden));
        }
        mobile.close_selected().unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(mobile.sessions().unwrap().iter().all(|s| s.id != session.id));
        let next_session = mobile.create_session(String::new()).unwrap();
        mobile.select(next_session.id, false).unwrap();
        mobile.disconnect().unwrap();
    }).await.unwrap();
    local
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn account_login_discovers_desktop_streams_input_and_revokes_active_connection() {
    use ai_terminal_security::account::DesktopAccountCommand;
    let (mut host, local) = host();
    let db = host.dir.join("account.db");
    ai_terminal_server::account::manage_user(&db, "test", "account test password", false).unwrap();
    let router =
        ai_terminal_server::router(&db, &ai_terminal_security::random_secret().unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let command = DesktopAccountCommand::Login {
        server: url.clone(),
        ca: None,
        username: "test".into(),
        password: "account test password".into(),
        device_name: "Integration Desktop".into(),
    };
    local
        .call(Request {
            operation: Operation::Account as i32,
            text: serde_json::to_string(&command).unwrap(),
            ..Request::default()
        })
        .unwrap();
    assert!(host.dir.join("account-mode").exists());
    let id = local
        .call(Request {
            operation: Operation::Create as i32,
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "stty -echo -icanon min 1 time 0; exec cat".into(),
            ],
            rows: 24,
            cols: 80,
            ..Request::default()
        })
        .unwrap()
        .info
        .unwrap()
        .id;
    attach_desktop(&local, &id);
    let selected = id.clone();
    tokio::task::spawn_blocking(move || {
        let account = ai_terminal_mobile::Account::new();
        account
            .login(
                url,
                "test".into(),
                "account test password".into(),
                "Integration Phone".into(),
                "ios".into(),
                String::new(),
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let desktop = loop {
            if let Some(device) = account
                .devices()
                .unwrap()
                .into_iter()
                .find(|d| d.platform == "desktop" && d.online)
            {
                break device;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(100));
        };
        let mobile = std::sync::Arc::new(ai_terminal_mobile::RemoteTerminal::new());
        account.connect(desktop.id.clone(), mobile.clone()).unwrap();
        let mut last = mobile.select(selected.clone(), true).unwrap();
        // The terminal program has no shell command interpretation; 1,000 visible x characters.
        thread::sleep(Duration::from_millis(100));
        for _ in 0..1000 {
            loop {
                match mobile.send_text("x".into(), false) {
                    Ok(()) => break,
                    Err(e) if e.to_string().contains("queue full") => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        }
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(frame) = mobile.refresh().unwrap() {
                last = frame;
            }
            let count = last.cells.iter().filter(|c| c.text == "x").count();
            if count == 1000 {
                break;
            }
            assert!(count <= 1000, "duplicate input {count}");
            assert!(Instant::now() < until, "missing input {count}");
            thread::sleep(Duration::from_millis(10));
        }
        let until = Instant::now() + Duration::from_secs(5);
        while mobile.connection_path() != "direct" && Instant::now() < until {
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(mobile.connection_path(), "direct");
        mobile.use_relay().unwrap();
        assert!(mobile.has_control());
        account.revoke(desktop.id).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while mobile.has_control() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!mobile.has_control());
        assert!(mobile.send_text("must not run".into(), false).is_err());
        mobile.disconnect().unwrap();
        account.logout().unwrap();
    })
    .await
    .unwrap();
    assert!(!local.call(Request::default()).unwrap().sessions[0].exited);
    local
        .call(Request {
            operation: Operation::Close as i32,
            session: id,
            ..Request::default()
        })
        .unwrap();
    local
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stream_reorders_window_replays_ack_and_fences_stale_input() {
    use ai_terminal_remote::StreamEvent;
    let (mut host, local) = host();
    let admin = ai_terminal_security::random_secret().unwrap();
    let router = ai_terminal_server::router(&host.dir.join("stream.db"), &admin).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let (pair, invite) = ai_terminal_remote::create_pair(&url, &admin, false)
        .await
        .unwrap();
    std::fs::create_dir_all(host.dir.join("pairs")).unwrap();
    std::fs::write(
        host.dir.join("pairs").join(format!("{}.json", pair.room)),
        serde_json::to_vec(&pair).unwrap(),
    )
    .unwrap();
    let mut channel = ai_terminal_remote::Channel::connect(&invite).await.unwrap();
    channel.negotiate_stream().await.unwrap();
    channel
        .stream_request(
            1,
            &Request {
                operation: Operation::Create as i32,
                rows: 24,
                cols: 80,
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "stty -echo -icanon min 1 time 0; exec cat".into(),
                ],
                ..Request::default()
            },
            false,
        )
        .await
        .unwrap();
    let info = loop {
        if let Some(StreamEvent::Reply(1, reply)) =
            channel.stream_next(Duration::from_secs(5)).await.unwrap()
        {
            assert!(reply.error.is_empty());
            break reply.info.unwrap();
        }
    };
    attach_desktop(&local, &info.id);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let first = Request {
        operation: Operation::Input as i32,
        session: info.id.clone(),
        session_epoch: info.epoch,
        control_epoch: info.control_epoch,
        input_seq: 1,
        input: b"x".to_vec(),
        ..Request::default()
    };
    let second = Request {
        input_seq: 2,
        input: b"y".to_vec(),
        ..first.clone()
    };
    channel.stream_request(3, &second, false).await.unwrap();
    channel.stream_request(2, &first, false).await.unwrap();
    channel.stream_request(2, &first, true).await.unwrap();
    let mut received = std::collections::HashSet::new();
    let until = Instant::now() + Duration::from_secs(5);
    while received.len() < 2 {
        if let Some(StreamEvent::Reply(id, reply)) = channel
            .stream_next(Duration::from_millis(50))
            .await
            .unwrap()
        {
            assert!(reply.error.is_empty());
            received.insert(id);
        }
        assert!(Instant::now() < until)
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let frame = local
        .call(Request {
            operation: Operation::Poll as i32,
            session: info.id.clone(),
            ..Request::default()
        })
        .unwrap()
        .snapshot
        .unwrap();
    assert_eq!(
        frame
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>()
            .trim(),
        "xy"
    );
    let local_info = local
        .call(Request {
            operation: Operation::Acquire as i32,
            session: info.id.clone(),
            ..Request::default()
        })
        .unwrap()
        .info
        .unwrap();
    local
        .call(Request {
            operation: Operation::Input as i32,
            session: info.id.clone(),
            control_epoch: local_info.control_epoch,
            input_seq: 1,
            input: b"w".to_vec(),
            ..Request::default()
        })
        .unwrap();
    channel
        .stream_request(
            4,
            &Request {
                input_seq: 3,
                input: b"z".to_vec(),
                ..first
            },
            false,
        )
        .await
        .unwrap();
    loop {
        if let Some(StreamEvent::Reply(4, reply)) =
            channel.stream_next(Duration::from_secs(5)).await.unwrap()
        {
            assert!(
                reply.error.is_empty(),
                "desktop attachment blocked mobile input: {}",
                reply.error
            );
            break;
        }
    }
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = local
            .call(Request {
                operation: Operation::Poll as i32,
                session: info.id.clone(),
                ..Request::default()
            })
            .unwrap()
            .snapshot
            .unwrap();
        if frame
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>()
            .trim()
            == "xywz"
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "desktop and mobile input did not reach one PTY"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    local
        .call(Request {
            operation: Operation::Close as i32,
            session: info.id,
            ..Request::default()
        })
        .unwrap();
    local
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn switching_accounts_cannot_adopt_previous_shells() {
    use ai_terminal_security::account::DesktopAccountCommand;
    let (mut host, local) = host();
    let db = host.dir.join("switch.db");
    for name in ["alice", "bob"] {
        ai_terminal_server::account::manage_user(&db, name, "account switch password", false)
            .unwrap();
    }
    let router =
        ai_terminal_server::router(&db, &ai_terminal_security::random_secret().unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let command = |value: DesktopAccountCommand| Request {
        operation: Operation::Account as i32,
        text: serde_json::to_string(&value).unwrap(),
        ..Request::default()
    };
    local
        .call(command(DesktopAccountCommand::Login {
            server: url.clone(),
            ca: None,
            username: "alice".into(),
            password: "account switch password".into(),
            device_name: "Desktop".into(),
        }))
        .unwrap();
    let id = local
        .call(Request {
            operation: Operation::Create as i32,
            rows: 24,
            cols: 80,
            ..Request::default()
        })
        .unwrap()
        .info
        .unwrap()
        .id;
    local.call(command(DesktopAccountCommand::Logout)).unwrap();
    local
        .call(command(DesktopAccountCommand::Login {
            server: url.clone(),
            ca: None,
            username: "bob".into(),
            password: "account switch password".into(),
            device_name: "Desktop".into(),
        }))
        .unwrap();
    let old = id.clone();
    tokio::task::spawn_blocking(move || {
        let account = ai_terminal_mobile::Account::new();
        account
            .login(
                url,
                "bob".into(),
                "account switch password".into(),
                "Phone".into(),
                "ios".into(),
                String::new(),
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let desktop = loop {
            if let Some(d) = account
                .devices()
                .unwrap()
                .into_iter()
                .find(|d| d.platform == "desktop" && d.online)
            {
                break d;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(50));
        };
        let terminal = std::sync::Arc::new(ai_terminal_mobile::RemoteTerminal::new());
        account.connect(desktop.id, terminal.clone()).unwrap();
        assert!(terminal.sessions().unwrap().is_empty());
        assert!(terminal.select(old, true).is_err());
        terminal.disconnect().unwrap();
        account.logout().unwrap();
    })
    .await
    .unwrap();
    assert_eq!(local.call(Request::default()).unwrap().sessions[0].id, id);
    local
        .call(Request {
            operation: Operation::Close as i32,
            session: id,
            ..Request::default()
        })
        .unwrap();
    local
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}
