use crate::scrollbar::Bar;
use ai_terminal_agent::Client;
use ai_terminal_protocol::{
    Snapshot,
    local::{Operation, Request, SCROLLBACK_BUSY, SCROLLBACK_EXPIRED},
};
use anyhow::{Context, Result};
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEventKind as MK};

#[derive(Default)]
pub struct Browser {
    pub frame: Option<Snapshot>,
    id: u64,
    offset: u32,
    unsupported: bool,
    total: u32,
    hover: bool,
    drag: Option<u16>,
    basis: Option<(u64, bool)>,
}
impl Browser {
    pub fn active(&self) -> bool {
        self.frame.is_some()
    }
    pub fn compatible(&self, live: &Snapshot) -> bool {
        self.basis
            .is_none_or(|basis| basis == (live.dimensions_epoch, live.alternate_screen))
    }
    pub fn leave(&mut self, client: &Client, request: &Request) -> Result<()> {
        if self.id != 0 {
            client.call(Request {
                operation: Operation::ReleaseScrollback as i32,
                scrollback_id: self.id,
                ..request.clone()
            })?;
        }
        self.id = 0;
        self.offset = 0;
        self.frame = None;
        self.total = 0;
        self.hover = false;
        self.drag = None;
        self.basis = None;
        Ok(())
    }
    pub fn scroll(
        &mut self,
        delta: i32,
        client: &Client,
        request: &Request,
        live: &Snapshot,
    ) -> Result<()> {
        if self.unsupported {
            return Ok(());
        }
        let offset = self.offset.saturating_add_signed(delta);
        if offset == 0 {
            return self.leave(client, request);
        }
        self.load(offset, client, request, live)
    }
    fn load(
        &mut self,
        offset: u32,
        client: &Client,
        request: &Request,
        live: &Snapshot,
    ) -> Result<()> {
        if self.unsupported {
            return Ok(());
        }
        let result = client.call(Request {
            operation: Operation::Scrollback as i32,
            scrollback_id: self.id,
            history_offset: offset,
            ..request.clone()
        });
        match result {
            Ok(reply) => {
                self.id = reply.scrollback_id;
                self.offset = reply.scrollback_offset;
                self.total = reply.scrollback_total;
                let mut frame = reply
                    .snapshot
                    .context("Agent omitted scrollback viewport")?;
                frame.validate()?;
                self.basis = Some((frame.dimensions_epoch, frame.alternate_screen));
                if self.offset == 0 {
                    self.frame = None;
                    return Ok(());
                }
                if reply.history_truncated && self.offset == reply.scrollback_total {
                    notice(
                        &mut frame,
                        "[Oldest retained output; earlier history unavailable]",
                    );
                }
                self.frame = Some(frame);
            }
            Err(error) => {
                let message = error.to_string();
                if message == "unknown operation" {
                    self.unsupported = true;
                    let mut frame = live.clone();
                    notice(
                        &mut frame,
                        "[Scrollback needs an updated Agent; existing sessions kept]",
                    );
                    self.frame = Some(frame);
                } else if message == SCROLLBACK_EXPIRED {
                    self.id = 0;
                    self.offset = 0;
                    self.frame = None;
                    self.total = 0;
                    self.drag = None;
                } else if message == SCROLLBACK_BUSY {
                    let mut frame = live.clone();
                    notice(
                        &mut frame,
                        "[History readers busy; return to live or try scrolling again]",
                    );
                    self.frame = Some(frame);
                } else {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub fn bar(&self, live: &Snapshot, size: (u16, u16)) -> Option<Bar> {
        if !self.hover && !self.active() && self.drag.is_none() {
            return None;
        }
        Bar::new(
            size.0.min(live.cols as u16),
            size.1.min(live.rows as u16),
            self.total,
            self.offset,
        )
    }
    pub fn mouse(
        &mut self,
        event: &Event,
        size: (u16, u16),
        watch: bool,
        client: &Client,
        request: &Request,
        live: &Snapshot,
    ) -> Result<bool> {
        let Event::Mouse(mouse) = event else {
            return Ok(false);
        };
        if matches!(mouse.kind, MK::Up(MouseButton::Left)) && self.drag.take().is_some() {
            if self.offset == 0 {
                self.leave(client, request)?;
            }
            return Ok(true);
        }
        if let Some(grab) = self.drag
            && matches!(mouse.kind, MK::Drag(MouseButton::Left))
        {
            if let Some(bar) = self.bar(live, size) {
                self.load(
                    bar.offset(mouse.row, grab, self.total),
                    client,
                    request,
                    live,
                )?;
            }
            return Ok(true);
        }
        let local = self.active()
            || watch
            || mouse.modifiers.contains(KeyModifiers::SHIFT)
            || (!live.alternate_screen && live.input_modes & 8 == 0);
        let cols = size.0.min(live.cols as u16);
        let rows = size.1.min(live.rows as u16);
        let edge = cols >= 2 && rows >= 2 && mouse.column == cols - 1 && mouse.row < rows;
        if matches!(mouse.kind, MK::Moved) {
            self.hover = local && edge;
            if self.hover && self.id == 0 {
                self.load(0, client, request, live)?;
            }
            if !self.hover && !self.active() && self.id != 0 {
                self.leave(client, request)?;
            }
            return Ok(self.hover);
        }
        if local && edge && matches!(mouse.kind, MK::Down(MouseButton::Left)) {
            self.hover = true;
            if !self.active() {
                self.id = 0;
                self.load(0, client, request, live)?;
            }
            if let Some(bar) = self.bar(live, size) {
                let grab = if (bar.top..bar.top + bar.height).contains(&mouse.row) {
                    mouse.row - bar.top
                } else {
                    bar.height / 2
                };
                self.drag = Some(grab);
                self.load(
                    bar.offset(mouse.row, grab, self.total),
                    client,
                    request,
                    live,
                )?;
            }
            return Ok(true);
        }
        Ok(false)
    }
}
fn notice(frame: &mut Snapshot, text: &str) {
    let start = (frame.rows as usize - 1) * frame.cols as usize;
    let mut chars = text.chars();
    for cell in &mut frame.cells[start..] {
        cell.text = chars.next().unwrap_or(' ').to_string();
        cell.width = 1;
        cell.foreground = 0xffffff;
        cell.background = 0x303030;
        cell.style = 0;
    }
    frame.cursor.as_mut().unwrap().visible = false;
    frame.seal();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use ai_terminal_protocol::local::{Reply, read_message, write_message};
    use std::{
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };

    #[test]
    fn old_agent_is_not_restarted_and_warns_only_once() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!(
            "aterminal-old-agent-{:x}",
            ai_terminal_agent::random_id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(directory.join("endpoint.json"), serde_json::to_vec(&serde_json::json!({"address":listener.local_addr().unwrap().to_string(),"token":"test"})).unwrap()).unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            while let Ok(request) = read_message::<_, Request>(&mut stream) {
                let reply = match Operation::try_from(request.operation).unwrap() {
                    Operation::List => Reply::default(),
                    Operation::Scrollback => {
                        count.fetch_add(1, Ordering::Relaxed);
                        Reply {
                            error: "unknown operation".into(),
                            ..Reply::default()
                        }
                    }
                    other => panic!("unexpected old Agent operation {other:?}"),
                };
                write_message(&mut stream, &reply).unwrap();
            }
        });
        let client = Client::connect(&directory).unwrap();
        let live = ai_terminal_engine::Engine::new(3, 80, 1)
            .unwrap()
            .snapshot();
        let request = Request::default();
        let mut browser = Browser::default();
        browser.scroll(3, &client, &request, &live).unwrap();
        let warning = browser.frame.as_ref().unwrap();
        warning.validate().unwrap();
        assert!(
            warning
                .cells
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .contains("updated Agent")
        );
        browser.leave(&client, &request).unwrap();
        browser.scroll(3, &client, &request, &live).unwrap();
        assert!(!browser.active());
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        drop(client);
        server.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
