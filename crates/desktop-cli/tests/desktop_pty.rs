//! Automated host/inner PTY coverage; no user shell configuration or GUI required.
#![cfg(unix)]
use ai_terminal_agent::Client;
use ai_terminal_engine::Engine;
use ai_terminal_protocol::local::{Operation, Request};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct State(PathBuf);
impl State {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "aterminal-host-{:x}",
            ai_terminal_agent::random_id()
        )))
    }
    fn client(&self) -> Client {
        Client::connect(&self.0).unwrap()
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Ok(client) = Client::connect(&self.0) {
            let _ = client.call(Request {
                operation: Operation::Shutdown as i32,
                ..Request::default()
            });
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Host {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: mpsc::Receiver<Vec<u8>>,
    engine: Engine,
    raw: Vec<u8>,
}
impl Host {
    fn start(state: &Path, args: &[&str], truecolor: bool) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 8,
                cols: 60,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_aTerminal"));
        command.arg("--state-dir");
        command.arg(state);
        command.args(args);
        command.env("TERM", "xterm-256color");
        command.env(
            "TERM_PROGRAM",
            if truecolor {
                "iTerm.app"
            } else {
                "Apple_Terminal"
            },
        );
        if truecolor {
            command.env("COLORTERM", "truecolor");
        } else {
            command.env_remove("COLORTERM");
        }
        command.env("AI_TERMINAL_CREDENTIAL_STORE", "file");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, output) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0; 65536];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master: pair.master,
            writer,
            output,
            engine: Engine::new(8, 60, 1).unwrap(),
            raw: Vec::new(),
        }
    }
    fn text(&self) -> String {
        self.engine
            .snapshot()
            .cells
            .iter()
            .filter(|c| c.width != 0)
            .map(|c| c.text.as_str())
            .collect()
    }
    fn pump(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            if let Ok(bytes) = self.output.recv_timeout(Duration::from_millis(10)) {
                self.raw.extend(&bytes);
                self.engine.feed(&bytes);
            }
        }
    }
    fn wait_for(&mut self, expected: &str) {
        let until = Instant::now() + Duration::from_secs(10);
        while !self.text().contains(expected) {
            self.pump(Duration::from_millis(20));
            assert!(
                Instant::now() < until,
                "missing {expected:?}; screen={:?}",
                self.text()
            );
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }
    fn wait_exit(&mut self) -> u32 {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            self.pump(Duration::from_millis(20));
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.exit_code();
            }
            assert!(Instant::now() < until, "CLI failed to exit");
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
const WORKLOAD: &str = r#"stty -echo; i=0; while [ $i -lt 80 ]; do printf '\033[31mROW_%03d 中é\033[0m\r\n' $i; i=$((i+1)); done; printf LIVE_READY; while IFS= read -r word; do case "$word" in alternate) printf '\033[?1049hALT_READY'; continue;; primary) printf '\033[?1049l';; esac; printf '\r\nREPLY_%s\r\n' "$word"; done"#;

#[test]
fn history_wheel_paging_input_watch_and_reattach() {
    let state = State::new();
    let mut host = Host::start(&state.0, &["--", "/bin/sh", "-c", WORKLOAD], false);
    host.wait_for("LIVE_READY");
    let client = state.client();
    let session = client.call(Request::default()).unwrap().sessions[0]
        .id
        .clone();
    let poll = Request {
        session: session.clone(),
        operation: Operation::Poll as i32,
        ..Request::default()
    };
    let live = client.call(poll.clone()).unwrap().snapshot.unwrap();
    host.send(b"\x1b[<64;10;4M");
    host.wait_for("ROW_070");
    assert!(!host.engine.snapshot().cursor.unwrap().visible);
    assert_eq!(client.call(poll.clone()).unwrap().snapshot.unwrap(), live);
    // Drag the internal right-edge thumb to the top, then below the track.
    host.send(b"\x1b[<0;60;7M\x1b[<32;60;1M\x1b[<0;60;1m");
    host.wait_for("ROW_000");
    host.send(b"\x1b[<0;60;1M\x1b[<32;60;24M\x1b[<0;60;24m");
    host.wait_for("LIVE_READY");
    // Reveal the bar from live mode and click its track.
    host.send(b"\x1b[<35;60;4M\x1b[<0;60;1M\x1b[<0;60;1m");
    host.wait_for("ROW_000");
    host.send(b"\x1b[5;2~".repeat(20).as_slice());
    host.wait_for("ROW_000");
    assert!(host.text().contains("中é"));
    assert!(host.raw.windows(6).any(|x| x == b";38;5;"));
    assert!(!host.raw.windows(6).any(|x| x == b";38;2;"));
    // A separate input client adds output while the Desktop is reading history.
    let acquired = client
        .call(Request {
            operation: Operation::Acquire as i32,
            ..poll.clone()
        })
        .unwrap()
        .info
        .unwrap();
    client
        .call(Request {
            operation: Operation::Input as i32,
            control_epoch: acquired.control_epoch,
            input_seq: acquired.next_input_seq,
            input: b"background\r".to_vec(),
            ..poll.clone()
        })
        .unwrap();
    host.pump(Duration::from_millis(150));
    assert!(host.text().contains("ROW_000"));
    assert!(!host.text().contains("REPLY_background"));
    host.send(b"typed\r");
    host.wait_for("REPLY_typed");
    assert!(host.text().contains("REPLY_background"));
    assert_eq!(host.text().matches("REPLY_typed").count(), 1);
    host.send(b"\x1b[5;2~".repeat(20).as_slice());
    host.wait_for("ROW_000");
    let control = client
        .call(Request {
            operation: Operation::Acquire as i32,
            ..poll.clone()
        })
        .unwrap()
        .info
        .unwrap();
    client
        .call(Request {
            operation: Operation::Input as i32,
            control_epoch: control.control_epoch,
            input_seq: control.next_input_seq,
            input: b"alternate\r".to_vec(),
            ..poll.clone()
        })
        .unwrap();
    host.wait_for("ALT_READY");
    assert!(!host.text().contains("ROW_000"));
    host.send(b"primary\r");
    host.wait_for("REPLY_primary");
    host.send(b"\x1d");
    assert_eq!(host.wait_exit(), 0);
    assert!(!host.engine.snapshot().alternate_screen);
    let mut watch = Host::start(&state.0, &["--attach", &session, "--watch"], true);
    watch.wait_for("REPLY_typed");
    watch.send(b"\x1b[5;2~".repeat(20).as_slice());
    watch.wait_for("ROW_000");
    let before = client.call(poll.clone()).unwrap().snapshot.unwrap();
    watch.send(b"must_not_write\r");
    watch.pump(Duration::from_millis(100));
    assert_eq!(client.call(poll.clone()).unwrap().snapshot.unwrap(), before);
    watch.send(b"\x1b[6;2~".repeat(20).as_slice());
    watch.wait_for("REPLY_typed");
    assert!(watch.raw.windows(6).any(|x| x == b";38;2;"));
    // A host resize exits a reading view even in read-only mode.
    watch.send(b"\x1b[5;2~".repeat(20).as_slice());
    watch.wait_for("ROW_000");
    watch.engine.resize(9, 62).unwrap();
    watch
        .master
        .resize(PtySize {
            rows: 9,
            cols: 62,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    watch.wait_for("REPLY_typed");
    watch.send(b"\x1d");
    assert_eq!(watch.wait_exit(), 0);
}

#[test]
fn startup_failure_restores_screen() {
    let state = State::new();
    let mut host = Host::start(&state.0, &["--", "/aterminal-test-does-not-exist"], false);
    assert_ne!(host.wait_exit(), 0);
    assert!(host.raw.windows(8).any(|x| x == b"\x1b[?1049h"));
    assert!(host.raw.windows(8).any(|x| x == b"\x1b[?1049l"));
    assert!(!host.engine.snapshot().alternate_screen);
    assert!(!host.raw.windows(4).any(|x| x == b"\x1b[3J"));
}
