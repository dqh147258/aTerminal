//! Readiness for isolated Windows daemons, without racing private state creation.
use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Reply, Request, read_message, write_message};
use anyhow::{Context, Result, ensure};
use std::{
    io::Read,
    net::{SocketAddr, TcpStream},
    path::Path,
    process::Child,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

pub fn wait_for_agent(child: &mut Child, root: &Path) -> Client {
    // Real private-ACL setup launches multiple PowerShell processes on Windows.
    // Bound the entire wait, including the final ACL-validating connection.
    let deadline = Instant::now() + Duration::from_secs(300);
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let captured = stderr.clone();
    let mut pipe = child
        .stderr
        .take()
        .expect("fixture must capture daemon stderr");
    thread::spawn(move || {
        let mut buffer = [0; 1024];
        let mut line = Vec::new();
        let mut oversized = false;
        while let Ok(n) = pipe.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let mut tail = captured.lock().unwrap();
            tail.extend_from_slice(&buffer[..n]);
            let excess = tail.len().saturating_sub(8192);
            drop(tail.drain(..excess));
            drop(tail);
            // These static stage lines exist only when daemon tracing is opted in.
            // Forward complete bounded lines; retain arbitrary stderr only in the tail.
            for &byte in &buffer[..n] {
                if byte == b'\n' {
                    if !oversized
                        && let Ok(text) = std::str::from_utf8(&line)
                        && let Some(stage) =
                            text.trim_end_matches('\r').strip_prefix("Agent startup: ")
                        && !stage.is_empty()
                        && stage
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b == b':' || b == b'-')
                    {
                        eprintln!("Agent startup: {stage}");
                    } else if !oversized
                        && let Ok(text) = std::str::from_utf8(&line)
                        && let Some((stage, utc)) = timing_marker(text.trim_end_matches('\r'))
                    {
                        eprintln!("Agent ACL timing: {stage} utc={utc}");
                    }
                    line.clear();
                    oversized = false;
                } else if !oversized {
                    if line.len() < 256 {
                        line.push(byte);
                    } else {
                        line.clear();
                        oversized = true;
                    }
                }
            }
        }
    });
    let phase = Arc::new(Mutex::new("waiting for endpoint publication".to_owned()));
    let progress = phase.clone();
    let directory = root.to_owned();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = wait_for_listener(&directory, deadline, &progress).and_then(|()| {
            *progress.lock().unwrap() = "validating private ACL and authenticated RPC".into();
            Client::connect(&directory)
        });
        let _ = tx.send(result);
    });
    loop {
        let diagnostic = || {
            format!(
                "{}; daemon stderr: {}",
                phase.lock().unwrap().as_str(),
                String::from_utf8_lossy(&stderr.lock().unwrap())
            )
        };
        if let Some(status) = child.try_wait().expect("inspect daemon process") {
            panic!(
                "isolated daemon exited during startup ({status}); {}",
                diagnostic()
            );
        }
        assert!(
            Instant::now() < deadline,
            "isolated daemon startup exceeded 300 seconds; {}",
            diagnostic()
        );
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(client)) => return client,
            Ok(Err(error)) => panic!(
                "isolated daemon startup failed: {error:#}; {}",
                diagnostic()
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("daemon readiness worker stopped; {}", diagnostic());
            }
        }
    }
}

fn wait_for_listener(root: &Path, deadline: Instant, phase: &Mutex<String>) -> Result<()> {
    let address = loop {
        ensure!(Instant::now() < deadline, "endpoint publication timed out");
        let address = std::fs::read(root.join("runtime/endpoint.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value["address"].as_str()?.parse::<SocketAddr>().ok());
        if let Some(address) = address {
            ensure!(
                address.ip().is_loopback(),
                "fixture endpoint is not loopback"
            );
            break address;
        }
        thread::sleep(Duration::from_millis(100));
    };
    *phase.lock().unwrap() = "waiting for initialized loopback listener".into();
    let remaining = deadline.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "listener startup timed out");
    let mut stream = TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(remaining.min(Duration::from_secs(1))))?;
    stream.set_read_timeout(Some(remaining))?;
    // The endpoint is written before account/config setup. An empty-token request
    // cannot dispatch an operation; its rejection proves the accept loop is ready.
    // Never extract, log, or send the endpoint token in this readiness probe.
    write_message(&mut stream, &Request::default())?;
    let reply: Reply = read_message(&mut stream).context("read daemon readiness response")?;
    ensure!(
        reply.error == "unauthorized local client",
        "unexpected daemon readiness response"
    );
    Ok(())
}

// Accept only fixed marker names and a complete UTC millisecond timestamp.
fn timing_marker(line: &str) -> Option<(&str, &str)> {
    let (stage, utc) = line
        .strip_prefix("Agent ACL timing: ")?
        .split_once(" utc=")?;
    if !matches!(
        stage,
        "entry" | "set-start" | "set-ready" | "get-start" | "get-ready" | "exit"
    ) {
        return None;
    }
    let bytes = utc.as_bytes();
    if bytes.len() != 24
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    Some((stage, utc))
}
