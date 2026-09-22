use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request};
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
    let dir = std::env::temp_dir().join(format!(
        "ai-terminal-test-{:x}",
        ai_terminal_agent::random_id()
    ));
    let child = Command::new(env!("CARGO_BIN_EXE_ai-terminal"))
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
    let mut stale = input;
    stale.input_seq = 2;
    assert!(client.call(stale).is_err());
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
            session: id,
            operation: Operation::Close as i32,
            ..Request::default()
        })
        .unwrap();
    assert!(second.call(Request::default()).unwrap().sessions.is_empty());
    second
        .call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })
        .unwrap();
    assert!(host.child.wait().unwrap().success());
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
    let (mut host, local) = host();
    let admin = ai_terminal_security::random_secret().unwrap();
    let router = ai_terminal_server::router(&host.dir.join("mobile.db"), &admin).unwrap();
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
    let invitation = invite.export().unwrap();
    let id = tokio::task::spawn_blocking(move || {
        let mobile = ai_terminal_mobile::RemoteTerminal::new();
        mobile.connect(invitation).unwrap();
        let session = mobile.create_session(String::new()).unwrap();
        mobile.select(session.id.clone(), true).unwrap();
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
        mobile.disconnect().unwrap();
        session.id
    })
    .await
    .unwrap();
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
        assert!(mobile.select(selected.clone(), true).is_err());
        mobile.select(selected, false).unwrap();
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
    local
        .call(Request {
            operation: Operation::Acquire as i32,
            session: info.id.clone(),
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
            assert!(reply.error.contains("control"));
            break;
        }
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
