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
fn attach_desktop(client: &Client, id: &str, epoch: u64) -> Result<bool> {
    match client.call(Request {
        session: id.into(),
        session_epoch: epoch,
        operation: Operation::AttachDesktop as i32,
        ..Request::default()
    }) {
        Ok(_) => Ok(false),
        Err(error) if error.to_string() == "unknown operation" => {
            client.call(Request {
                session: id.into(),
                session_epoch: epoch,
                operation: Operation::Acquire as i32,
                ..Request::default()
            })?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}
pub fn run(args: Args) -> Result<u32> {
    let state_dir = args.state_dir.map(Ok).unwrap_or_else(default_state_dir)?;
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
        if args.json {
            println!("{}", serde_json::json!({"ok":true}));
        }
        return Ok(0);
    }
    if args.list {
        let sessions = client.call(Request::default())?.sessions;
        if args.json {
            let rows:Vec<_>=sessions.iter().map(|s|serde_json::json!({"id":s.id,"epoch":s.epoch,"initial_cwd":s.cwd,"exited":s.exited,"desktop_attached":s.desktop_attached})).collect();
            println!("{}", serde_json::json!({"ok":true,"result":rows}));
            return Ok(0);
        }
        for s in sessions {
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
        if args.json {
            println!("{}", serde_json::json!({"ok":true}));
        }
        return Ok(0);
    }
    if let Some(id) = args.history {
        let r = client.call(Request {
            session: id,
            operation: Operation::History as i32,
            history_limit: 200,
            ..Request::default()
        })?;
        if args.json {
            println!(
                "{}",
                serde_json::json!({"ok":true,"result":{"lines":r.history,"truncated":r.history_truncated}})
            );
            return Ok(0);
        }
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
    let (reply, legacy_agent) = if let Some(id) = args.attach {
        let legacy = if args.watch {
            false
        } else {
            attach_desktop(&client, &id, 0)?
        };
        (
            client.call(Request {
                session: id,
                operation: Operation::Poll as i32,
                ..Request::default()
            })?,
            legacy,
        )
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
        let created = client.call(Request {
            operation: Operation::Create as i32,
            shell_integration: args.shell_integration,
            cwd,
            command,
            rows: u32::from(rows),
            cols: u32::from(cols),
            ..Request::default()
        })?;
        if args.watch {
            (created, false)
        } else {
            let created_info = created
                .info
                .as_ref()
                .context("Agent omitted session identity")?;
            let legacy = attach_desktop(&client, &created_info.id, created_info.epoch)?;
            (
                client.call(Request {
                    session: created_info.id.clone(),
                    session_epoch: created_info.epoch,
                    operation: Operation::Poll as i32,
                    ..Request::default()
                })?,
                legacy,
            )
        }
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
    let interactive = !args.watch;
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
                    Ok(mut info) => {
                        if interactive
                            && !legacy_agent
                            && !stop.load(Ordering::Acquire)
                            && !info.desktop_attached
                        {
                            match polling.call(Request {
                                session: poll_id.clone(),
                                session_epoch: epoch,
                                operation: Operation::AttachDesktop as i32,
                                ..Request::default()
                            }) {
                                Ok(reply) => {
                                    if let Some(attached) = reply.info {
                                        info = attached
                                    }
                                }
                                Err(e) => {
                                    shared.lock().unwrap().error = Some(e.to_string());
                                    break;
                                }
                            }
                            if stop.load(Ordering::Acquire) {
                                let _ = polling.call(Request {
                                    session: poll_id.clone(),
                                    session_epoch: epoch,
                                    operation: Operation::Detach as i32,
                                    ..Request::default()
                                });
                                break;
                            }
                        }
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
            if is_detach_key(&event) {
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
fn is_detach_key(event: &Event) -> bool {
    matches!(event, Event::Key(key)
        if key.kind != KeyEventKind::Release
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char(']') | KeyCode::Char('5')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn raw_ctrl_bracket_alias_detaches() {
        for character in [']', '5'] {
            assert!(is_detach_key(&Event::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::CONTROL
            ))));
        }
    }
}
