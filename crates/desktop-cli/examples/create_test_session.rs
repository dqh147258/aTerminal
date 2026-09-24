//! Create a detached Desktop PTY for native-device integration tests.
use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Context, Result};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let [_, state_dir, cwd, columns, command @ ..] = args.as_slice() else {
        anyhow::bail!("usage: create_test_session STATE_DIR CWD COLS [COMMAND ...]")
    };
    let client = Client::connect(Path::new(state_dir))?;
    let reply = client.call(Request {
        operation: Operation::Create as i32,
        cwd: cwd.clone(),
        rows: 24,
        cols: columns.parse()?,
        command: command.to_vec(),
        ..Request::default()
    })?;
    let info = reply.info.context("create omitted session")?;
    client.call(Request {
        operation: Operation::Detach as i32,
        session: info.id.clone(),
        session_epoch: info.epoch,
        ..Request::default()
    })?;
    println!("AIT_TERMINAL_TEST_SESSION={}", info.id);
    Ok(())
}
