mod account;
mod agents;
mod configuration;
mod extensions;
mod input;
mod managed;
mod render;
use ai_terminal_agent::pty as session;
use ai_terminal_engine::Engine;
use anyhow::{Context, Result, bail};
use clap::Parser;
use session::{Output, Session};
use std::{
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};

/// A managed terminal with one authoritative screen. P0 local prototype.
#[derive(Parser)]
#[command(name = "aTerminal", version)]
pub(crate) struct Args {
    #[command(subcommand)]
    management: Option<account::Management>,
    #[arg(long, hide = true)]
    agent: bool,
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long)]
    list: bool,
    #[arg(long)]
    attach: Option<String>,
    #[arg(long)]
    watch: bool,
    #[arg(long)]
    close: Option<String>,
    #[arg(long)]
    history: Option<String>,
    #[arg(long)]
    agent_stop: bool,
    /// Create a device invitation; admin token comes from --server-token-file.
    #[arg(long)]
    pair: bool,
    #[arg(long, requires = "pair")]
    server: Option<String>,
    #[arg(long, requires = "pair")]
    server_token_file: Option<PathBuf>,
    #[arg(long, requires = "pair")]
    read_only: bool,
    /// Revoke a local pair and terminate its active relay task.
    #[arg(long)]
    revoke_pair: Option<String>,
    /// Start in this directory, without shell interpolation.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Enable isolated per-session shell status hooks.
    #[arg(long)]
    shell_integration: bool,
    /// Capture a validated protobuf screen without entering a host terminal.
    #[arg(long)]
    snapshot: Option<PathBuf>,
    #[arg(long, default_value_t = 24)]
    rows: u16,
    #[arg(long, default_value_t = 80)]
    cols: u16,
    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,
    /// Executable and arguments; defaults to the user's interactive shell.
    #[arg(last = true)]
    command: Vec<OsString>,
}
fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            if std::env::args().any(|arg| arg == "--json") {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"error":{"code":"operation_failed","message":format!("{error:#}")}})
                );
            } else {
                eprintln!("aTerminal: {error:#}");
            }
            1
        }
    };
    std::process::exit(code as i32);
}
fn run() -> Result<u32> {
    let mut args = Args::parse();
    if let Some(command) = args.management.take() {
        anyhow::ensure!(
            !args.list
                && args.attach.is_none()
                && args.close.is_none()
                && args.history.is_none()
                && !args.agent_stop
                && !args.agent,
            "management commands cannot be mixed with legacy terminal flags"
        );
        match command {
            account::Management::Sessions { command } => match command {
                account::Sessions::Capture { id, output } => {
                    use ai_terminal_protocol::local::{Operation, Request};
                    use std::io::Write;
                    let root = args
                        .state_dir
                        .map(Ok)
                        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
                    let reply = ai_terminal_agent::Client::connect(&root)?.call(Request {
                        operation: Operation::ObserveTerminal as i32,
                        session: id,
                        ..Default::default()
                    })?;
                    let frame = reply.snapshot.context("snapshot_unavailable")?;
                    let captured = ai_terminal_agent::raster::capture(&frame)?;
                    let mut file = std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&output)?;
                    file.write_all(&captured.png)?;
                    file.sync_all()?;
                    println!(
                        "{}",
                        serde_json::json!({"ok":true,"result":{"output":output,"source":"rendered_terminal","epoch":frame.epoch,"revision":frame.revision}})
                    );
                    return Ok(0);
                }
                account::Sessions::List => args.list = true,
                account::Sessions::Attach { id } => args.attach = Some(id),
                account::Sessions::Close { id } => args.close = Some(id),
                account::Sessions::History { id } => args.history = Some(id),
                account::Sessions::Show { id } => {
                    use ai_terminal_protocol::local::{Operation, Request};
                    let root = args
                        .state_dir
                        .map(Ok)
                        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
                    let reply = ai_terminal_agent::Client::connect(&root)?.call(Request {
                        operation: Operation::Poll as i32,
                        session: id,
                        ..Default::default()
                    })?;
                    let info = reply.info.context("session_not_found")?;
                    println!(
                        "{}",
                        serde_json::json!({"ok":true,"result":{"id":info.id,"epoch":info.epoch,"initial_cwd":info.cwd,"exited":info.exited,"exit_code":if info.exited{Some(info.exit_code)}else{None},"desktop_attached":info.desktop_attached}})
                    );
                    return Ok(0);
                }
            },
            account::Management::Daemon {
                command: account::Daemon::Stop,
            } => args.agent_stop = true,
            command => return account::run(command, args.state_dir, args.json),
        }
    }
    if args.agent {
        ai_terminal_agent::run_agent(
            &args
                .state_dir
                .clone()
                .map(Ok)
                .unwrap_or_else(ai_terminal_agent::default_state_dir)?,
        )?;
        return Ok(0);
    }
    if args.snapshot.is_some() {
        return capture(args);
    }
    if args.pair || args.revoke_pair.is_some() {
        return pairing(args);
    }
    managed::run(args)
}

fn pairing(args: Args) -> Result<u32> {
    let state = args
        .state_dir
        .map(Ok)
        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
    ai_terminal_agent::Client::ensure(&state, &std::env::current_exe()?)?;
    let pairs = state.join("pairs");
    std::fs::create_dir_all(&pairs)?;
    if let Some(id) = args.revoke_pair {
        anyhow::ensure!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "invalid pair ID"
        );
        std::fs::remove_file(pairs.join(format!("{id}.json")))?;
        println!("Pair revoked: {id}");
        return Ok(0);
    }
    let server = args
        .server
        .context("--pair requires --server https://your-server")?;
    let token = std::fs::read_to_string(
        args.server_token_file
            .context("--pair requires --server-token-file")?,
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (host, invitation) = runtime.block_on(ai_terminal_remote::create_pair(
        &server,
        &token,
        args.read_only,
    ))?;
    let path = pairs.join(format!("{}.json", host.room));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    serde_json::to_writer(file, &host)?;
    println!("{}", invitation.export()?);
    eprintln!(
        "Pair ID: {}. Import this invitation only on the intended device.",
        host.room
    );
    Ok(0)
}

fn capture(args: Args) -> Result<u32> {
    if args.command.is_empty() {
        bail!("--snapshot requires an explicit command after --")
    }
    let mut engine = Engine::new(args.rows, args.cols, 1)?;
    let mut session = Session::spawn(&args.command, args.cwd.as_deref(), args.rows, args.cols)?;
    let deadline = Instant::now() + Duration::from_secs(args.timeout_secs);
    loop {
        if Instant::now() >= deadline {
            bail!("capture timed out; terminating child")
        }
        match session.output.recv_timeout(Duration::from_millis(20)) {
            Ok(Output::Bytes(bytes)) => {
                for response in engine.feed(&bytes) {
                    session.write(response)?
                }
            }
            Ok(Output::Error(e)) => bail!("PTY read failed: {e}"),
            Ok(Output::End) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                for response in engine.tick() {
                    session.write(response)?
                }
            }
        }
        session.check_writer()?;
    }
    engine.finish();
    let frame = engine.snapshot();
    frame.validate()?;
    std::fs::write(args.snapshot.expect("capture has path"), frame.wire())
        .context("write snapshot")?;
    loop {
        if let Some(status) = session.exit_status()? {
            return Ok(status.exit_code());
        }
        if Instant::now() >= deadline {
            bail!("child did not exit before capture deadline")
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
