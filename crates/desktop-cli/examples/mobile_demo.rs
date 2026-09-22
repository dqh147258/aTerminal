//! Create an isolated local demo session and pair for simulator verification.
use ai_terminal_agent::Client;
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Context, Result, ensure};
use std::{fs::OpenOptions, path::PathBuf};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let root = PathBuf::from(args.get(1).context(
        "usage: mobile_demo STATE_DIR ADMIN_TOKEN_FILE CLI_BINARY [SERVER] [VERIFY_OUTPUT]",
    )?);
    let binary = PathBuf::from(args.get(3).context("CLI binary required")?).canonicalize()?;
    let client = Client::ensure(&root, &binary)?;
    let server = args.get(4).map_or("http://127.0.0.1:8787", String::as_str);
    let manifest = root.join("demo.json");
    if manifest.exists() {
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest)?)?;
        ensure!(
            saved["server"].as_str() == Some(server),
            "state directory belongs to a different endpoint"
        );
        let id = saved["session_id"]
            .as_str()
            .context("missing saved session")?;
        let reply = client.call(Request {
            session: id.into(),
            operation: Operation::Poll as i32,
            ..Request::default()
        })?;
        ensure!(
            !reply.info.context("missing session status")?.exited,
            "saved demo session exited; choose a new state directory"
        );
        if let Some(expected) = args.get(5) {
            let frame = reply.snapshot.context("missing terminal frame")?;
            ensure!(
                frame.cells.chunks(frame.cols as usize).any(|row| row
                    .iter()
                    .filter(|c| c.width > 0)
                    .map(|c| c.text.as_str())
                    .collect::<String>()
                    .trim()
                    == expected),
                "expected output has not reached the desktop PTY"
            );
            println!(
                "PASS: iOS input produced the expected independent output line in session {id}"
            );
        } else {
            println!("Reusing demo session={id}; state={}", root.display());
        }
        return Ok(());
    }
    let dir = root.join("pairs");
    std::fs::create_dir_all(&dir)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let invitation_path = root.join("invitation.txt");
    let invite = if invitation_path.exists() {
        let invite =
            ai_terminal_security::Invitation::import(&std::fs::read_to_string(&invitation_path)?)?;
        ensure!(invite.server == server, "invitation endpoint mismatch");
        ensure!(
            dir.join(format!("{}.json", invite.room)).exists(),
            "invitation has no matching desktop pair"
        );
        invite
    } else {
        let token = std::fs::read_to_string(args.get(2).context("admin token file required")?)?;
        let (pair, invite) = ai_terminal_remote::create_pair(server, token.trim(), false).await?;
        serde_json::to_writer(
            options.open(dir.join(format!("{}.json", pair.room)))?,
            &pair,
        )?;
        invite
    };
    use std::io::Write;
    if !invitation_path.exists() {
        options
            .open(&invitation_path)?
            .write_all(invite.export()?.as_bytes())?;
    }
    #[cfg(unix)]let command=vec!["/bin/sh".into(),"-c".into(),r"printf '\033[32mAI Terminal remote session\033[0m\r\nDesktop PTY connected\r\n'; exec /bin/sh -i".into()];
    #[cfg(windows)]
    let command = vec!["powershell.exe".into(), "-NoProfile".into()];
    let reply = client.call(Request {
        operation: Operation::Create as i32,
        command,
        rows: 24,
        cols: 80,
        ..Request::default()
    })?;
    let id = reply.info.context("missing session")?.id;
    client.call(Request {
        session: id.clone(),
        operation: Operation::Detach as i32,
        ..Request::default()
    })?;
    serde_json::to_writer_pretty(
        options.open(&manifest)?,
        &serde_json::json!({"server":server,"session_id":id,"pair_id":invite.room,"state_dir":root}),
    )?;
    println!(
        "demo session={id}; invitation saved privately under {}",
        root.display()
    );
    Ok(())
}
