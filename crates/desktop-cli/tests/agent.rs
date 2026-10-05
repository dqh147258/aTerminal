#[cfg(windows)]
#[path = "support/windows_daemon.rs"]
mod windows_daemon;

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
    let mut command = Command::new(env!("CARGO_BIN_EXE_aTerminal"));
    #[cfg(windows)]
    command.env("ATERMINAL_STARTUP_TRACE", "1");
    command
        .env("AI_TERMINAL_LEGACY_ASSISTANT", "1")
        .env_remove("AI_TERMINAL_AI_BASE_URL")
        .env_remove("AI_TERMINAL_AI_MODEL")
        .env_remove("AI_TERMINAL_AI_API_KEY");
    if let Some(url) = model_url {
        command
            .env("AI_TERMINAL_LEGACY_ASSISTANT", "1")
            .env("AI_TERMINAL_AI_BASE_URL", url)
            .env("AI_TERMINAL_AI_MODEL", "test");
    }
    let child = command
        .env("AI_TERMINAL_CREDENTIAL_STORE", "file")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("HOME", &dir)
        .args(["--agent", "--state-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if cfg!(windows) {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .spawn()
        .unwrap();
    let mut host = Host { child, dir };
    #[cfg(windows)]
    {
        let client = windows_daemon::wait_for_agent(&mut host.child, &host.dir);
        (host, client)
    }
    #[cfg(not(windows))]
    {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(c) = Client::connect(&host.dir) {
                return (host, c);
            }
            assert!(
                host.child.try_wait().unwrap().is_none(),
                "Agent exited during startup"
            );
            assert!(Instant::now() < until, "Agent startup timed out");
            thread::sleep(Duration::from_millis(20));
        }
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
        #[cfg(unix)]
        {
            mobile.send_text("i=0; while [ $i -lt 450 ]; do printf 'HISTORY_%03d\\n' $i; i=$((i+1)); done; echo MOBILE_RESUMED".into(), true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let reply = desktop.call(Request { session: session.id.clone(), operation: Operation::Poll as i32, ..Request::default() }).unwrap();
                if let Some(frame) = reply.snapshot && has_output(frame.cells.iter().map(|c| c.text.as_str()), frame.cols as usize, "HISTORY_449") { break; }
                assert!(Instant::now() < deadline, "history fixture output missing");
                thread::sleep(Duration::from_millis(20));
            }
        }
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
        let live = mobile.refresh().unwrap().unwrap();
        let viewport = mobile.read_history_viewport(None, u32::MAX).unwrap();
        assert!(viewport.total > 0);
        assert_eq!(viewport.cursor.offset, viewport.total);
        assert!(!viewport.frame.cursor_visible);
        assert!(
            mobile.refresh().unwrap().is_none(),
            "history changed the live replica"
        );
        assert_ne!(
            live.cells
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>(),
            viewport
                .frame
                .cells
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
        );
        let mut stale = viewport.cursor.clone();
        stale.session = "wrong-session".into();
        assert!(mobile.read_history_viewport(Some(stale), 1).is_err());
        mobile.release_history(viewport.cursor).unwrap();
        let first = mobile.read_history_page(None).unwrap();
        assert!(!first.lines.is_empty());
        assert!(first.total >= first.cursor.offset);
        let mut cursor = first.cursor;
        let mut more = first.has_more;
        let mut loaded = first.lines.len() as u32;
        let mut lines = first.lines.clone();
        while more {
            let page = mobile.read_history_page(Some(cursor)).unwrap();
            loaded += page.lines.len() as u32;
            lines.splice(0..0, page.lines.clone());
            assert_eq!(loaded, page.cursor.offset);
            more = page.has_more;
            cursor = page.cursor;
        }
        assert_eq!(loaded, first.total);
        #[cfg(unix)]
        {
            assert!(first.total > 450);
            let markers: Vec<_> = lines
                .iter()
                .filter(|line| line.starts_with("HISTORY_"))
                .collect();
            assert_eq!(markers.len(), 450);
            assert_eq!(markers[0].as_str(), "HISTORY_000");
            assert_eq!(markers[449].as_str(), "HISTORY_449");
        }
        mobile.release_history(cursor).unwrap();
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

#[test]
fn configuration_rpc_is_revisioned_and_provider_secrets_are_write_only() {
    let (mut host, client) = host();
    let call = |value: serde_json::Value| {
        client.call(Request {
            operation: Operation::Configuration as i32,
            text: value.to_string(),
            ..Default::default()
        })
    };
    let malformed =
        call(serde_json::json!({"action":"test-provider-write-only-secret"})).unwrap_err();
    assert!(
        !malformed
            .to_string()
            .contains("test-provider-write-only-secret")
    );
    let initial = call(serde_json::json!({"action":"show"})).unwrap();
    let initial: serde_json::Value = serde_json::from_str(&initial.history[0]).unwrap();
    let revision = initial["revision"].as_u64().unwrap();
    let config = serde_json::json!({"providers":{"local":{"id":"local","name":"Local","connection":{"protocol":"openai_chat","endpoint":"http://127.0.0.1:1","api_version":null},"catalog_url":null,"secret_ref":null,"credential_revision":0,"enabled":true}},"models":{},"bindings":{}});
    let request = serde_json::json!({"action":"replace","expected_revision":revision,"config":config,"secrets":{"local":"test-provider-write-only-secret"}});
    let saved = call(request.clone()).unwrap();
    assert!(!saved.history[0].contains("test-provider-write-only-secret"));
    assert!(call(request).is_err());
    let saved: serde_json::Value = serde_json::from_str(&saved.history[0]).unwrap();
    assert_eq!(saved["revision"], revision + 1);
    assert!(saved["config"]["providers"]["local"]["secret_ref"].is_string());
    client
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Default::default()
        })
        .unwrap();
    host.child.wait().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistent_agent_reads_analyzes_then_inputs_through_real_mcp_and_pty() {
    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    async fn model(
        State(calls): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> impl axum::response::IntoResponse {
        let mut calls = calls.lock().unwrap();
        let n = calls.len();
        calls.push(body);
        let delta = match n {
            0 => {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_read","type":"function","function":{"name":"read_terminal","arguments":"{\"mode\":\"tail\",\"max_lines\":5}"}}]})
            }
            1 => {
                json!({"content":json!({"summary":"Captured bounded terminal text","key_quotes":[],"facts":[],"tui_lines":[],"open_questions":[],"observed_status":"unknown"}).to_string()})
            }
            2 => {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_input","type":"function","function":{"name":"input_text","arguments":json!({"text":if cfg!(windows){"Write-Output ('NEW_AGENT_' + 'OK')"}else{"printf 'NEW_AGENT_%s\\n' OK"},"submit":true}).to_string()}}]})
            }
            _ => json!({"content":"Command was queued; this is not proof of command completion."}),
        };
        let first = json!({"id":format!("response-{n}"),"object":"chat.completion.chunk","created":0,"model":"fake","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        let end = json!({"id":format!("response-{n}"),"object":"chat.completion.chunk","created":0,"model":"fake","choices":[{"index":0,"delta":{},"finish_reason":if n==0||n==2{"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}});
        (
            [("content-type", "text/event-stream")],
            format!("data: {first}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
        )
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/chat/completions", post(model))
        .with_state(calls.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (mut host, client) = host();
    let created = client
        .call(Request {
            operation: Operation::Create as i32,
            rows: 8,
            cols: 80,
            command: if cfg!(windows) {
                vec![
                    "powershell.exe".into(),
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                ]
            } else {
                vec!["/bin/sh".into()]
            },
            ..Default::default()
        })
        .unwrap();
    let session = created.info.unwrap().id;
    attach_desktop(&client, &session);
    let initial = client
        .call(Request {
            operation: Operation::Configuration as i32,
            text: json!({"action":"show"}).to_string(),
            ..Default::default()
        })
        .unwrap();
    let initial: Value = serde_json::from_str(&initial.history[0]).unwrap();
    let config = json!({"providers":{"fake":{"id":"fake","name":"Fake","connection":{"protocol":"openai_chat","endpoint":format!("http://{address}"),"api_version":null},"catalog_url":null,"secret_ref":null,"credential_revision":0,"enabled":true}},"models":{"fake":{"id":"fake","name":"Fake","provider_id":"fake","model":"fake","context_window":128000,"max_tokens":2000,"temperature":null,"top_p":null,"reasoning":{"mode":"provider_default"},"capabilities":{"tools":true,"streaming":true},"max_rounds":8,"max_seconds":30,"read_only":false}},"bindings":{"session-default":{"model_id":"fake","reasoning":null}}});
    client
        .call(Request {
            operation: Operation::Configuration as i32,
            text:
                json!({"action":"replace","expected_revision":initial["revision"],"config":config})
                    .to_string(),
            ..Default::default()
        })
        .unwrap();
    let rpc = |mut value: Value| {
        value["version"] = json!(1);
        let reply = client
            .call(Request {
                operation: Operation::Agent as i32,
                session: session.clone(),
                text: value.to_string(),
                ..Default::default()
            })
            .unwrap();
        serde_json::from_str::<Value>(&reply.history[0]).unwrap()
    };
    let request = ai_terminal_agent_runtime::request_id();
    let sent = rpc(
        json!({"action":"send","request_id":request,"message":"Read the terminal, then print the marker once.","allow_input":true}),
    );
    assert_eq!(sent["state"], "running");
    // Legacy allow_input enables asking; it must never silently approve a write.
    let until = Instant::now() + Duration::from_secs(20);
    let waiting = loop {
        let state = rpc(json!({"action":"state"}));
        if state["state"] == "waiting_for_user" {
            break state;
        }
        assert_eq!(state["state"], "running", "{state}");
        assert!(Instant::now() < until, "{state}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert_eq!(waiting["permissions"]["permission_mode"], "ask");
    assert_eq!(waiting["permissions"]["full_authorization"], false);
    let pending = waiting["pending"]["items"].as_array().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["kind"], "approval");
    assert_eq!(pending[0]["tool"], "input_text");
    assert_eq!(pending[0]["state"], "pending");
    assert_eq!(calls.lock().unwrap().len(), 3);
    let frame = client
        .call(Request {
            operation: Operation::Poll as i32,
            session: session.clone(),
            ..Default::default()
        })
        .unwrap()
        .snapshot
        .unwrap();
    let text = frame
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>();
    assert!(!text.contains("NEW_AGENT_OK"));

    let approval = json!({
        "action":"resolve",
        "request_id":ai_terminal_agent_runtime::request_id(),
        "pending_id":pending[0]["id"],
        "decision":"once"
    });
    assert_eq!(rpc(approval.clone())["duplicate"], false);
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let state = rpc(json!({"action":"state"}));
        if state["state"] == "completed" {
            break;
        }
        assert_ne!(state["state"], "paused", "{state}");
        assert!(Instant::now() < until, "{state}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let history = rpc(json!({"action":"history"}));
    let items = history["items"].as_array().unwrap();
    let record = items
        .iter()
        .filter_map(|i| i["value"]["updates"].as_array())
        .flatten()
        .find_map(|u| u["record_id"].as_str())
        .unwrap()
        .to_owned();
    assert!(items.iter().any(|i| i["kind"] == "interaction"));
    assert_eq!(calls.lock().unwrap().len(), 4);
    assert_eq!(rpc(approval)["duplicate"], true);
    let permissions = rpc(json!({"action":"permissions"}));
    assert_eq!(permissions["permission_mode"], "ask");
    assert_eq!(permissions["full_authorization"], false);
    assert_eq!(rpc(json!({"action":"rules"}))["items"], json!([]));

    {
        let requests = calls.lock().unwrap();
        let original = requests[0]["messages"].as_array().unwrap();
        let analysis = requests[1]["messages"].as_array().unwrap();
        // Action-stage guidance is request-local, not persisted conversation history.
        let (action, history) = original.split_last().unwrap();
        assert_eq!(action["role"], "user");
        assert!(
            action["content"]
                .as_str()
                .unwrap()
                .starts_with("Application action stage:")
        );
        assert_eq!(analysis.len(), history.len() + 3);
        assert_eq!(&analysis[..history.len()], history);
        assert!(!analysis.iter().any(|message| message == action));
        let read_call = &analysis[history.len()];
        assert_eq!(read_call["role"], "assistant");
        assert_eq!(read_call["tool_calls"][0]["id"], "call_read");
        assert_eq!(
            read_call["tool_calls"][0]["function"]["name"],
            "read_terminal"
        );
        let observation = &analysis[history.len() + 1];
        assert_eq!(observation["role"], "tool");
        assert_eq!(observation["tool_call_id"], "call_read");
        assert!(observation.to_string().contains(&record));
        for index in [2, 3] {
            let messages = requests[index]["messages"].as_array().unwrap();
            assert_eq!(messages.last().unwrap(), action);
        }
        for key in ["model", "temperature", "max_tokens", "tools", "tool_choice"] {
            assert_eq!(requests[0][key], requests[1][key], "analysis changed {key}");
        }
        assert!(
            analysis
                .last()
                .unwrap()
                .to_string()
                .contains("Application analysis stage")
        );
    }
    let raw = rpc(json!({"action":"record","record_id":record,"part":"body"}));
    assert_eq!(raw["record_id"], record);
    rpc(
        json!({"action":"send","request_id":request,"message":"Read the terminal, then print the marker once.","allow_input":true}),
    );
    assert_eq!(calls.lock().unwrap().len(), 4);
    loop {
        let frame = client
            .call(Request {
                operation: Operation::Poll as i32,
                session: session.clone(),
                ..Default::default()
            })
            .unwrap()
            .snapshot
            .unwrap();
        let text = frame
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>();
        if text.contains("NEW_AGENT_OK") {
            break;
        }
        assert!(Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    client
        .call(Request {
            operation: Operation::Close as i32,
            session: session.clone(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        rpc(json!({"action":"record","record_id":record,"part":"body"}))["record_id"],
        record
    );
    client
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Default::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}

#[cfg(unix)]
#[test]
fn opt_in_shell_hooks_report_exit_and_cwd_without_global_rc_changes() {
    let (mut host, client) = host();
    // Keep this isolated hook test independent of Ubuntu's completion audit prompt.
    std::fs::write(host.dir.join(".zshenv"), "skip_global_compinit=1\n").unwrap();
    for shell in ["/bin/bash", "/bin/zsh"] {
        if !std::path::Path::new(shell).exists() {
            continue;
        }
        let created = client
            .call(Request {
                operation: Operation::Create as i32,
                command: vec![shell.into()],
                shell_integration: true,
                cwd: host.dir.to_string_lossy().into_owned(),
                rows: 8,
                cols: 80,
                ..Default::default()
            })
            .unwrap();
        let id = created.info.unwrap().id;
        attach_desktop(&client, &id);
        let info;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let r = client
                .call(Request {
                    operation: Operation::Poll as i32,
                    session: id.clone(),
                    ..Default::default()
                })
                .unwrap();
            let next = r.info.unwrap();
            let status: serde_json::Value =
                serde_json::from_str(&next.shell_status).unwrap_or_default();
            if status["phase"] == "prompt"
                && status["sequence"] == 0
                && status["evidence_source"] == "session_shell_hook"
            {
                info = next;
                break;
            }
            assert!(
                Instant::now() < deadline,
                "shell hook did not report: {shell}"
            );
            thread::sleep(Duration::from_millis(50));
        }
        client
            .call(Request {
                operation: Operation::Input as i32,
                session: id.clone(),
                control_epoch: info.control_epoch,
                input_seq: info.next_input_seq,
                input_kind: 1,
                text: "false".into(),
                submit: true,
                ..Default::default()
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let r = client
                .call(Request {
                    operation: Operation::Poll as i32,
                    session: id.clone(),
                    ..Default::default()
                })
                .unwrap();
            let value: serde_json::Value =
                serde_json::from_str(&r.info.unwrap().shell_status).unwrap_or_default();
            if value["phase"] == "prompt" && value["exit_code"] == 1 {
                assert_eq!(value["sequence"], 1);
                assert_eq!(value["command"], "false");
                assert_eq!(value["command_association"], true);
                assert_eq!(value["trusted_for_authorization"], false);
                assert_eq!(
                    std::path::Path::new(value["cwd"].as_str().unwrap())
                        .canonicalize()
                        .unwrap(),
                    host.dir.canonicalize().unwrap()
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "exit status missing: {shell}: {value}"
            );
            thread::sleep(Duration::from_millis(50));
        }
        client
            .call(Request {
                operation: Operation::Close as i32,
                session: id,
                ..Default::default()
            })
            .unwrap();
    }
    client
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Default::default()
        })
        .unwrap();
    host.child.wait().unwrap();
}

/// Explicit opt-in only: consumes real provider quota and sends synthetic PTY text.
/// MODELSCOPE_API_KEY_FILE is a local credential file, not committed test data.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit real ModelScope credentials and network access"]
async fn modelscope_live_terminal_agent() {
    use serde_json::{Value, json};
    let key = std::fs::read_to_string(
        std::env::var("MODELSCOPE_API_KEY_FILE").expect("set MODELSCOPE_API_KEY_FILE"),
    )
    .unwrap();
    let key = key.trim();
    assert!(!key.is_empty());
    let endpoint = std::env::var("MODELSCOPE_BASE_URL")
        .unwrap_or_else(|_| "https://api-inference.modelscope.cn/v1".into());
    let model = std::env::var("MODELSCOPE_MODEL").unwrap_or_else(|_| "Qwen/Qwen3.8-27B".into());
    // A transparent test-only streaming relay captures the exact request and SSE
    // received from the real provider. Authorization headers are never recorded.
    type WireTrace =
        std::sync::Arc<std::sync::Mutex<Vec<(Value, std::sync::Arc<std::sync::Mutex<Vec<u8>>>)>>>;
    #[derive(Clone)]
    struct LiveProxy {
        client: reqwest::Client,
        endpoint: String,
        key: String,
        trace: WireTrace,
    }
    async fn forward(
        axum::extract::State(proxy): axum::extract::State<LiveProxy>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        use futures_util::StreamExt;
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        proxy
            .trace
            .lock()
            .unwrap()
            .push((body.clone(), captured.clone()));
        let response = match proxy
            .client
            .post(format!(
                "{}/chat/completions",
                proxy.endpoint.trim_end_matches('/')
            ))
            .bearer_auth(&proxy.key)
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return (
                    axum::http::StatusCode::BAD_GATEWAY,
                    axum::Json(json!({"error":error.to_string()})),
                )
                    .into_response();
            }
        };
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_owned();
        let stream = response.bytes_stream().map(move |chunk| {
            let bytes = chunk.map_err(std::io::Error::other)?;
            let mut saved = captured.lock().unwrap();
            if saved.len() + bytes.len() > 4 * 1024 * 1024 {
                return Err(std::io::Error::other("live_trace_limit"));
            }
            saved.extend_from_slice(&bytes);
            Ok(bytes)
        });
        axum::response::Response::builder()
            .status(status)
            .header("content-type", content_type)
            .body(axum::body::Body::from_stream(stream))
            .unwrap()
    }
    let wire_trace: WireTrace = Default::default();
    let proxy = LiveProxy {
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
        endpoint: endpoint.clone(),
        key: key.to_owned(),
        trace: wire_trace.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .route("/chat/completions", axum::routing::post(forward))
                .with_state(proxy),
        )
        .await
        .unwrap()
    });
    let (mut host, client) = host();
    let seed = format!("SOURCE_{:016x}", ai_terminal_agent::random_id());
    let marker = format!("RESULT_{:016x}", ai_terminal_agent::random_id());
    let created = client
        .call(Request {
            operation: Operation::Create as i32,
            cwd: host.dir.to_string_lossy().into_owned(),
            rows: 12,
            cols: 100,
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                format!("printf '{seed}\\n'; exec /bin/sh -i"),
            ],
            ..Default::default()
        })
        .unwrap();
    let session = created.info.unwrap().id;
    attach_desktop(&client, &session);
    let configuration = json!({
        "providers":{"modelscope":{"id":"modelscope","name":"ModelScope live test","connection":{"protocol":"openai_chat","endpoint":proxy_endpoint},"credential_revision":0,"enabled":true}},
        "models":{"qwen-live":{"id":"qwen-live","name":"Qwen live test","provider_id":"modelscope","model":model,"context_window":65536,"max_tokens":4096,"reasoning":{"mode":"provider_default"},"capabilities":{"tools":true,"streaming":true,"source":"explicit_live_validation"},"max_rounds":16,"max_seconds":300,"read_only":false}},
        "bindings":{"global":{"model_id":"qwen-live"},"session-default":{"model_id":"qwen-live"}}
    });
    client.call(Request {
        operation: Operation::Configuration as i32,
        text: json!({"action":"replace","expected_revision":0,"config":configuration,"secrets":{"modelscope":key}}).to_string(),
        ..Default::default()
    }).unwrap();
    let rpc = |session: &str, mut value: Value| {
        value["version"] = json!(1);
        let result = client
            .call(Request {
                operation: Operation::Agent as i32,
                session: session.into(),
                text: value.to_string(),
                ..Default::default()
            })
            .unwrap();
        serde_json::from_str::<Value>(&result.history[0]).unwrap()
    };
    // Let the synthetic source reach the authoritative terminal before authorizing work.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let reply = client
            .call(Request {
                operation: Operation::Poll as i32,
                session: session.clone(),
                ..Default::default()
            })
            .unwrap();
        let text = reply
            .snapshot
            .unwrap()
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>();
        if text.contains(&seed) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fixture PTY did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let request_id = ai_terminal_agent_runtime::request_id();
    let message = format!(
        "这是已授权的隔离终端自动化测试。请严格依次完成：1. 调用 read_terminal(mode=tail,max_lines=20) 读取终端中的 SOURCE_ 标记。2. 完成应用要求的分析后，调用 input_text(text=\"printf '{marker}\\n' | tee -a live-marker.txt\",submit=true)，只执行这一次写入。3. 再次调用 read_terminal 确认输出。4. 最后回复 LIVE_OK 和你最初实际读到的完整 SOURCE_ 标记。每轮只调用一个工具；不要执行其他写操作，也不要委托。应用的分析阶段按其要求返回 JSON。不要将输入入队当作完成证据。"
    );
    let send =
        json!({"action":"send","request_id":request_id,"message":message,"allow_input":true});
    let accepted = rpc(&session, send.clone());
    let start = Instant::now();
    let final_state = loop {
        let state = rpc(&session, json!({"action":"state"}));
        client
            .call(Request {
                operation: Operation::Poll as i32,
                session: session.clone(),
                revision: u64::MAX,
                ..Default::default()
            })
            .unwrap();
        if !["running", "stopping", "finishing"].contains(&state["state"].as_str().unwrap_or("")) {
            break state;
        }
        if start.elapsed() > Duration::from_secs(330) {
            rpc(&session, json!({"action":"cancel"}));
            panic!("real provider test timed out");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    let history = rpc(&session, json!({"action":"history"}));
    let database = rusqlite::Connection::open_with_flags(
        host.dir.join("data/agent.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let usage = database
        .prepare("SELECT stage,value FROM model_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (stage, value) = row.unwrap();
            json!({"stage":stage,"usage":serde_json::from_str::<Value>(&value).unwrap()})
        })
        .collect::<Vec<_>>();
    let call_count = usage.len();
    let retry = rpc(&session, send);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after_retry: i64 = database
        .query_row("SELECT COUNT(*) FROM model_usage", [], |row| row.get(0))
        .unwrap();
    let marker_text = std::fs::read_to_string(host.dir.join("live-marker.txt")).unwrap_or_default();
    let records = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["value"]["updates"].as_array())
        .flatten()
        .filter_map(|update| update["record_id"].as_str())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let originals = records
        .iter()
        .map(|id| {
            rpc(
                &session,
                json!({"action":"record","record_id":id,"part":"body"}),
            )
        })
        .collect::<Vec<_>>();
    let anchor_records = records
        .iter()
        .map(|id| {
            rpc(
                &session,
                json!({"action":"record","record_id":id,"part":"anchors"}),
            )
        })
        .collect::<Vec<_>>();
    let executed_writes: i64 = database
        .query_row(
            "SELECT COUNT(*) FROM actions WHERE state IN ('written','accepted')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let trace=wire_trace.lock().unwrap().iter().map(|(request,response)|json!({"request":request,"response_sse":String::from_utf8_lossy(&response.lock().unwrap())})).collect::<Vec<_>>();
    let report = json!({"provider":"modelscope","endpoint":endpoint,"model":model,"elapsed_ms":start.elapsed().as_millis(),"accepted":accepted,"state":final_state,"request_retry":retry,"model_calls":call_count,"calls_after_retry":after_retry,"usage":usage,"marker_lines":marker_text.lines().collect::<Vec<_>>(),"expected_marker":marker,"expected_source":seed,"history":history,"originals":originals,"anchors":anchor_records,"executed_writes":executed_writes,"http_trace":trace});
    if let Ok(path) = std::env::var("MODELSCOPE_LIVE_REPORT") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(&report)
                .unwrap()
                .replace(key, "[redacted]"),
        )
        .unwrap();
    }
    eprintln!(
        "ModelScope live: state={}, model_calls={}, elapsed={:?}",
        final_state["state"],
        call_count,
        start.elapsed()
    );
    assert_eq!(
        final_state["state"],
        "completed",
        "{}",
        final_state.to_string().replace(key, "[redacted]")
    );
    assert!(
        usage.iter().any(|u| u["stage"] == "analysis"),
        "no actual observation analysis occurred"
    );
    assert_eq!(
        marker_text.lines().collect::<Vec<_>>(),
        vec![marker.as_str()],
        "terminal write was missing or repeated"
    );
    assert!(
        originals
            .iter()
            .any(|v| v["body"].as_str().is_some_and(|s| s.contains(&seed)))
    );
    assert!(history["items"].as_array().unwrap().iter().any(|item| {
        item["kind"] == "assistant"
            && item["value"]["text"]
                .as_str()
                .is_some_and(|s| s.contains("LIVE_OK") && s.contains(&seed))
    }));
    assert_eq!(
        executed_writes, 1,
        "unexpected additional side-effecting tools"
    );
    for anchor in &anchor_records {
        assert_eq!(anchor["metadata"]["head_lines"], 10);
        assert_eq!(anchor["metadata"]["tail_lines"], 20);
        let tui = anchor["tui_lines"].as_array().unwrap();
        for edge in ["search_head_anchor", "search_tail_anchor"] {
            assert!(
                anchor[edge]["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|line| !tui.contains(line))
            );
        }
    }
    assert!(
        anchor_records
            .iter()
            .any(|a| !a["tui_lines"].as_array().unwrap().is_empty()),
        "model did not classify the interactive prompt"
    );
    assert_eq!(retry["duplicate"], true);
    assert_eq!(call_count as i64, after_retry);
    assert_eq!(
        wire_trace.lock().unwrap().len(),
        call_count,
        "retry caused another HTTP request"
    );
    let first = &trace[0]["request"];
    let prefix = first["messages"].as_array().unwrap();
    for exchange in &trace {
        let request = &exchange["request"];
        let messages = request["messages"].as_array().unwrap();
        if messages.last().is_some_and(|m| {
            m["content"]
                .as_str()
                .is_some_and(|text| text.starts_with("Application analysis stage"))
        }) {
            assert_eq!(&messages[..prefix.len()], prefix.as_slice());
            for field in ["model", "temperature", "max_tokens", "tools", "tool_choice"] {
                assert_eq!(
                    first[field], request[field],
                    "live analysis changed {field}"
                );
            }
        }
    }
    client
        .call(Request {
            operation: Operation::Close as i32,
            session: session.clone(),
            ..Default::default()
        })
        .unwrap();
    for id in &records {
        assert_eq!(
            rpc(
                &session,
                json!({"action":"record","record_id":id,"part":"body"})
            )["record_id"],
            *id
        );
    }
    client
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Default::default()
        })
        .unwrap();
    host.child.wait().unwrap();
    server.abort();
}
