use crate::{
    Args, input,
    render::{Renderer, TerminalGuard},
};
use ai_terminal_agent::{Client, default_state_dir};
use ai_terminal_protocol::{
    ProtocolError, Replica, Snapshot,
    local::{Operation, Reply, Request, SESSION_CLOSED_ERROR, SessionInfo},
};
use anyhow::{Context, Result, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

struct View {
    frame: Arc<Snapshot>,
    info: SessionInfo,
    error: Option<String>,
}
struct Attachment {
    client: Client,
    id: String,
    stop: Arc<AtomicBool>,
}
impl Drop for Attachment {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.client.call(Request {
            session: self.id.clone(),
            operation: Operation::Detach as i32,
            ..Request::default()
        });
    }
}
pub fn run(args: Args) -> Result<u32> {
    let state_dir = args.state_dir.unwrap_or_else(default_state_dir);
    let client = if args.list || args.close.is_some() || args.history.is_some() || args.agent_stop {
        Client::connect(&state_dir).context("no running Agent")?
    } else {
        Client::ensure(&state_dir, &std::env::current_exe()?)?
    };
    if args.agent_stop {
        client.call(Request {
            operation: Operation::Shutdown as i32,
            ..Request::default()
        })?;
        return Ok(0);
    }
    if args.list {
        for s in client.call(Request::default())?.sessions {
            println!(
                "{}\t{}\t{}",
                s.id,
                if s.exited { "exited" } else { "running" },
                s.cwd
            )
        }
        return Ok(0);
    }
    if let Some(id) = args.close {
        client.call(Request {
            session: id,
            operation: Operation::Close as i32,
            ..Request::default()
        })?;
        return Ok(0);
    }
    if let Some(id) = args.history {
        let r = client.call(Request {
            session: id,
            operation: Operation::History as i32,
            history_limit: 200,
            ..Request::default()
        })?;
        for line in r.history {
            println!("{line}")
        }
        if r.history_truncated {
            eprintln!("[earlier history omitted]");
        }
        return Ok(0);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("attach requires a terminal; --snapshot captures non-interactively")
    }
    let (cols, rows) = terminal::size()?;
    let reply = if let Some(id) = args.attach {
        if !args.watch {
            client.call(Request {
                session: id.clone(),
                operation: Operation::Acquire as i32,
                ..Request::default()
            })?;
        }
        client.call(Request {
            session: id,
            operation: Operation::Poll as i32,
            ..Request::default()
        })?
    } else {
        let cwd = args.cwd.unwrap_or(std::env::current_dir()?);
        let cwd = cwd
            .to_str()
            .context("session path must be UTF-8")?
            .to_owned();
        let command = args
            .command
            .into_iter()
            .map(|s| {
                s.into_string()
                    .map_err(|_| anyhow::anyhow!("command must be UTF-8"))
            })
            .collect::<Result<Vec<_>>>()?;
        client.call(Request {
            operation: Operation::Create as i32,
            cwd,
            command,
            rows: u32::from(rows),
            cols: u32::from(cols),
            ..Request::default()
        })?
    };
    let info = reply.info.context("Agent omitted session identity")?;
    let frame = reply.snapshot.context("Agent omitted initial snapshot")?;
    frame.validate()?;
    let id = info.id.clone();
    let epoch = info.epoch;
    let control_epoch = info.control_epoch;
    let mut seq = info.next_input_seq;
    let state = Arc::new(Mutex::new(View {
        frame: Arc::new(frame.clone()),
        info,
        error: None,
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let _attachment = Attachment {
        client: client.clone(),
        id: id.clone(),
        stop: stop.clone(),
    };
    if !args.watch {
        client.call(Request {
            session: id.clone(),
            session_epoch: epoch,
            operation: Operation::Resize as i32,
            rows: rows.into(),
            cols: cols.into(),
            control_epoch,
            ..Request::default()
        })?;
    }
    let polling = client.clone();
    let shared = state.clone();
    let poll_id = id.clone();
    thread::spawn(move || {
        let mut replica = Replica::default();
        replica.snapshot(frame).expect("validated frame");
        let mut force_snapshot = false;
        while !stop.load(Ordering::Acquire) {
            let revision = if force_snapshot {
                0
            } else {
                replica.state().map_or(0, |s| s.revision)
            };
            let result = polling.call(Request {
                session: poll_id.clone(),
                session_epoch: epoch,
                operation: Operation::Poll as i32,
                revision,
                ..Request::default()
            });
            match result {
                Ok(reply) => match update(&mut replica, reply) {
                    Ok(info) => {
                        force_snapshot = false;
                        let mut view = shared.lock().unwrap();
                        let current = replica.state().unwrap();
                        if view.frame.revision != current.revision {
                            view.frame = Arc::new(current.clone());
                        }
                        view.info = info;
                    }
                    Err(ProtocolError::Baseline) => force_snapshot = true,
                    Err(e) => {
                        shared.lock().unwrap().error = Some(e.to_string());
                        break;
                    }
                },
                Err(e) => {
                    let mut view = shared.lock().unwrap();
                    if e.to_string() == SESSION_CLOSED_ERROR {
                        view.info.exited = true;
                        view.info.exit_code = 0;
                    } else {
                        view.error = Some(e.to_string());
                    }
                    break;
                }
            }
            thread::sleep(Duration::from_millis(3));
        }
    });
    let _terminal = TerminalGuard::enter()?;
    let mut renderer = Renderer::default();
    renderer.viewport(cols, rows);
    let mut out = io::stdout().lock();
    loop {
        let (frame, info, error) = {
            let view = state.lock().unwrap();
            (view.frame.clone(), view.info.clone(), view.error.clone())
        };
        if let Some(e) = error {
            bail!("Agent connection ended: {e}; session may still be running")
        }
        if renderer.revision() != frame.revision {
            renderer.draw(&mut out, (*frame).clone())?;
        }
        if info.exited {
            return Ok(info.exit_code);
        }
        if !info.error.is_empty() {
            bail!("session error: {}", info.error)
        }
        if event::poll(Duration::from_millis(3))? {
            let event = event::read()?;
            if let Event::Resize(cols, rows) = &event {
                renderer.viewport(*cols, *rows);
            }
            if matches!(&event,Event::Key(k) if k.kind!=KeyEventKind::Release&&k.code==KeyCode::Char(']')&&k.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(0);
            }
            if args.watch {
                continue;
            }
            match event {
                Event::Resize(cols, rows) => {
                    client.call(Request {
                        session: id.clone(),
                        session_epoch: epoch,
                        operation: Operation::Resize as i32,
                        rows: rows.into(),
                        cols: cols.into(),
                        control_epoch,
                        ..Request::default()
                    })?;
                }
                e => {
                    if let Some(bytes) = input::encode(e, frame.input_modes) {
                        client.call(Request {
                            session: id.clone(),
                            session_epoch: epoch,
                            operation: Operation::Input as i32,
                            control_epoch,
                            input_seq: seq,
                            input: bytes,
                            ..Request::default()
                        })?;
                        seq += 1;
                    }
                }
            }
        }
    }
}
fn update(replica: &mut Replica, reply: Reply) -> Result<SessionInfo, ProtocolError> {
    if let Some(snapshot) = reply.snapshot {
        replica.snapshot(snapshot)?;
    }
    if let Some(delta) = reply.delta {
        replica.delta(delta)?;
    }
    reply.info.ok_or(ProtocolError::Invalid)
}
