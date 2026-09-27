use crate::{
    CoreError, DisplayBatch, RenderFrame, RenderPatch, RenderUpdate, render_cell, render_frame,
};
use ai_terminal_protocol::{
    Replica,
    local::{Operation, Reply, Request, SessionInfo},
};
use ai_terminal_remote::{Channel, PathKind, StreamEvent};
use ai_terminal_security::Invitation;
use anyhow::{Context, Result, bail, ensure};
use prost::Message;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, OnceLock, TryLockError},
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .expect("mobile network runtime")
    })
}
pub(crate) fn ffi(e: impl std::fmt::Display) -> CoreError {
    CoreError::InvalidFrame {
        reason: e.to_string(),
    }
}
#[derive(uniffi::Record)]
pub struct RemoteSession {
    pub id: String,
    pub cwd: String,
    pub exited: bool,
    pub exit_code: u32,
    pub desktop_attached: bool,
}
#[derive(Clone, uniffi::Record)]
pub struct TerminalHistoryCursor {
    pub session: String,
    pub epoch: u64,
    pub view_id: u64,
    pub offset: u32,
}
#[derive(uniffi::Record)]
pub struct TerminalHistoryPage {
    pub cursor: TerminalHistoryCursor,
    pub lines: Vec<String>,
    pub total: u32,
    pub has_more: bool,
    pub truncated: bool,
}
#[derive(uniffi::Record)]
pub struct TerminalHistoryViewport {
    pub cursor: TerminalHistoryCursor,
    pub frame: RenderFrame,
    pub total: u32,
}
struct State {
    replica: Replica,
    desired: Option<String>,
    selected: Option<SessionInfo>,
    controlled: bool,
    connected: bool,
    path: String,
    error: Option<String>,
    next_input: u64,
    generation: u64,
    dirty: BTreeSet<usize>,
    full: bool,
    changed: bool,
    last_frame: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            replica: Replica::default(),
            desired: None,
            selected: None,
            controlled: false,
            connected: true,
            path: "relay".into(),
            error: None,
            next_input: 1,
            generation: 0,
            dirty: BTreeSet::new(),
            full: true,
            changed: false,
            last_frame: 0,
        }
    }
}
impl State {
    fn request(&self, op: Operation) -> Result<Request> {
        let s = self.selected.as_ref().context("select a session first")?;
        ensure!(self.connected, "offline; reconnect before typing");
        Ok(Request {
            session: s.id.clone(),
            session_epoch: s.epoch,
            control_epoch: s.control_epoch,
            operation: op as i32,
            ..Request::default()
        })
    }
    fn apply(&mut self, reply: &Reply) -> Result<()> {
        if let Some(info) = &reply.info {
            if self.desired.as_ref().is_some_and(|id| id != &info.id) {
                return Ok(());
            }
            // Control metadata and screen revisions have independent ordering. A later input
            // reply can advance the control epoch before an earlier valid display delta arrives.
            let current_metadata = self.selected.as_ref().is_none_or(|old| {
                info.control_epoch > old.control_epoch
                    || info.control_epoch == old.control_epoch
                        && info.availability_epoch >= old.availability_epoch
            });
            if current_metadata {
                if let Some(old) = &self.selected
                    && (old.control_epoch != info.control_epoch || info.exited)
                {
                    self.controlled = false;
                }
                let mut info = info.clone();
                if let Some(old) = &self.selected
                    && old.control_epoch == info.control_epoch
                {
                    info.next_input_seq = info.next_input_seq.max(old.next_input_seq);
                }
                self.controlled = info.controller != 0 && desktop_available(&info) && !info.exited;
                self.selected = Some(info);
            }
        }
        if let Some(s) = &reply.snapshot
            && self.replica.snapshot(s.clone())?
        {
            self.full = true;
            self.changed = true;
            self.dirty.clear();
        }
        if let Some(d) = &reply.delta
            && self.replica.delta(d.clone())?
        {
            self.changed = true;
            for patch in &d.patches {
                self.dirty.insert(patch.index as usize);
            }
        }
        Ok(())
    }
}
struct Job {
    request: Request,
    response: Option<oneshot::Sender<Result<Reply, String>>>,
    queued: Instant,
}
struct Pending {
    job: Job,
    sent: Instant,
    retried: bool,
}
struct Worker {
    tx: mpsc::Sender<Job>,
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(uniffi::Object, Default)]
pub struct RemoteTerminal {
    inner: Mutex<Option<Worker>>,
}
impl RemoteTerminal {
    pub(crate) fn set_channel(&self, mut channel: Channel) -> Result<(), CoreError> {
        runtime()
            .block_on(channel.negotiate_stream())
            .map_err(ffi)?;
        let (tx, rx) = mpsc::channel(64);
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        let task = runtime().spawn(async move {
            if let Err(e) = pump(channel, rx, shared.clone()).await {
                let mut state = shared.lock().unwrap();
                state.connected = false;
                state.controlled = false;
                state.error = Some(e.to_string());
                state.path = "offline".into();
            }
        });
        *self.inner.lock().map_err(ffi)? = Some(Worker { tx, state, task });
        Ok(())
    }
    fn shared(&self) -> Result<(mpsc::Sender<Job>, Arc<Mutex<State>>), CoreError> {
        let inner = self.inner.lock().map_err(ffi)?;
        let w = inner.as_ref().ok_or_else(|| ffi("not connected"))?;
        Ok((w.tx.clone(), w.state.clone()))
    }
    fn management(
        &self,
        operation: Operation,
        session: String,
        text: String,
        limit: usize,
    ) -> Result<String, CoreError> {
        if session.len() > 128 || text.len() > limit {
            return Err(ffi("management_request_limit"));
        }
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| ffi("invalid_management_request"))?;
        if operation == Operation::Agent && value["version"] != 1 {
            return Err(ffi("unsupported_agent_version"));
        }
        let reply = self.call(Request {
            operation: operation as i32,
            session,
            text,
            ..Default::default()
        })?;
        reply
            .history
            .into_iter()
            .next()
            .ok_or_else(|| ffi("desktop_feature_unavailable"))
    }
    fn call(&self, request: Request) -> Result<Reply, CoreError> {
        let (tx, state) = self.shared()?;
        if !state.lock().map_err(ffi)?.connected {
            return Err(ffi("offline"));
        }
        let (send, receive) = oneshot::channel();
        tx.try_send(Job {
            request,
            response: Some(send),
            queued: Instant::now(),
        })
        .map_err(|_| ffi("request queue full or closed"))?;
        runtime().block_on(receive).map_err(ffi)?.map_err(ffi)
    }
    fn input(&self, fill: impl FnOnce(&mut Request)) -> Result<(), CoreError> {
        let (tx, state) = self.shared()?;
        let mut state = state.lock().map_err(ffi)?;
        if !state.controlled || !state.connected {
            return Err(ffi("terminal is currently read-only"));
        }
        // Capture session identity, fencing epoch and sequence under one lock. Selection cannot
        // change between building the request and reserving its sequence number.
        let mut request = state.request(Operation::Input).map_err(ffi)?;
        fill(&mut request);
        request.input_seq = state.next_input;
        tx.try_send(Job {
            request,
            response: None,
            queued: Instant::now(),
        })
        .map_err(|_| ffi("input queue full; input was not accepted"))?;
        state.next_input += 1;
        Ok(())
    }
}
#[uniffi::export]
impl RemoteTerminal {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn connect(&self, invitation: String) -> Result<(), CoreError> {
        self.disconnect()?;
        let invite = Invitation::import(&invitation).map_err(ffi)?;
        let channel = runtime().block_on(Channel::connect(&invite)).map_err(ffi)?;
        self.set_channel(channel)
    }
    pub fn disconnect(&self) -> Result<(), CoreError> {
        if let Ok((_, state)) = self.shared() {
            let request = {
                let s = state.lock().map_err(ffi)?;
                if s.controlled {
                    s.request(Operation::Detach).ok()
                } else {
                    None
                }
            };
            if let Some(request) = request {
                let _ = self.call(request);
            }
        }
        *self.inner.lock().map_err(ffi)? = None;
        Ok(())
    }
    pub fn sessions(&self) -> Result<Vec<RemoteSession>, CoreError> {
        Ok(self
            .call(Request::default())?
            .sessions
            .into_iter()
            .map(session)
            .collect())
    }
    /// Agent requests remain independent of the selected terminal input lease.
    pub fn agent(&self, session_id: String, request_json: String) -> Result<String, CoreError> {
        self.management(Operation::Agent, session_id, request_json, 65536)
    }
    pub fn configuration(&self, request_json: String) -> Result<String, CoreError> {
        self.management(
            Operation::Configuration,
            String::new(),
            request_json,
            1024 * 1024,
        )
    }
    pub fn assistant(&self, session_id: String, request_json: String) -> Result<String, CoreError> {
        if session_id.is_empty() || session_id.len() > 128 || request_json.len() > 16000 {
            return Err(ffi("invalid assistant request"));
        }
        let value: serde_json::Value = serde_json::from_str(&request_json).map_err(ffi)?;
        let allow_input = value["action"] == "send" && value["allow_input"] == true;
        let (_, shared) = self.shared()?;
        if allow_input {
            let controlled = {
                let state = shared.lock().map_err(ffi)?;
                if state.selected.as_ref().map(|s| s.id.as_str()) != Some(session_id.as_str()) {
                    return Err(ffi("select this terminal before asking AI to operate it"));
                }
                state.controlled
            };
            if !controlled {
                self.select(session_id.clone(), true)?;
            }
        }
        let state = shared.lock().map_err(ffi)?;
        let (control_epoch, input_seq) = if allow_input {
            let selected = state
                .selected
                .as_ref()
                .ok_or_else(|| ffi("no selected terminal"))?;
            if selected.id != session_id || !state.controlled {
                return Err(ffi("terminal control changed"));
            }
            (selected.control_epoch, state.next_input)
        } else {
            (0, 0)
        };
        drop(state);
        let reply = self.call(Request {
            session: session_id,
            operation: Operation::Assistant as i32,
            text: request_json,
            control_epoch,
            input_seq,
            ..Request::default()
        })?;
        reply
            .history
            .into_iter()
            .next()
            .ok_or_else(|| ffi("Desktop does not support assistant requests"))
    }
    pub fn select(&self, id: String, take_control: bool) -> Result<RenderFrame, CoreError> {
        let (_, state) = self.shared()?;
        let detach = {
            let s = state.lock().map_err(ffi)?;
            if s.controlled {
                s.request(Operation::Detach).ok()
            } else {
                None
            }
        };
        if let Some(req) = detach {
            self.call(req)?;
        }
        {
            let mut s = state.lock().map_err(ffi)?;
            s.controlled = false;
            s.selected = None;
            s.desired = Some(id.clone());
            s.replica.reset();
            s.generation += 1;
            s.dirty.clear();
            s.full = true;
            s.changed = false;
            s.last_frame = 0;
        }
        let acquired = if take_control {
            match self.call(Request {
                session: id.clone(),
                operation: Operation::Acquire as i32,
                ..Request::default()
            }) {
                Ok(reply) => reply.info,
                Err(CoreError::InvalidFrame { reason })
                    if reason.contains("read-only permission")
                        || reason.contains("session exited") =>
                {
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        self.call(Request {
            session: id,
            operation: Operation::Subscribe as i32,
            ..Request::default()
        })?;
        let mut s = state.lock().map_err(ffi)?;
        s.controlled = acquired
            .as_ref()
            .zip(s.selected.as_ref())
            .is_some_and(|(a, b)| {
                a.id == b.id
                    && a.control_epoch == b.control_epoch
                    && a.controller == b.controller
                    && desktop_available(b)
                    && !b.exited
            });
        s.next_input = s.selected.as_ref().map_or(1, |s| s.next_input_seq);
        Ok(render_frame(
            s.replica
                .state()
                .ok_or_else(|| ffi("missing initial snapshot"))?,
        ))
    }
    /// Compatibility full-frame read; consumes no network round trip.
    pub fn refresh(&self) -> Result<Option<RenderFrame>, CoreError> {
        let (_, state) = self.shared()?;
        let mut s = state.lock().map_err(ffi)?;
        if let Some(e) = &s.error {
            return Err(ffi(e));
        }
        let Some(frame) = s.replica.state() else {
            return Ok(None);
        };
        if frame.revision == s.last_frame {
            return Ok(None);
        }
        let frame = render_frame(frame);
        s.last_frame = frame.revision;
        Ok(Some(frame))
    }
    /// Read a coherent display/control batch without waiting for the replica lock. None means
    /// busy; all dirty cells remain queued for the next poll, not acknowledged or discarded here.
    pub fn poll_display(&self) -> Result<Option<DisplayBatch>, CoreError> {
        let state = match self.inner.try_lock() {
            Ok(inner) => inner
                .as_ref()
                .ok_or_else(|| ffi("not connected"))?
                .state
                .clone(),
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(e) => return Err(ffi(e)),
        };
        let mut state = match state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(e) => return Err(ffi(e)),
        };
        let update = take_update(&mut state)?;
        Ok(Some(DisplayBatch {
            update,
            controlled: state.connected && state.controlled,
            desktop_attached: state.selected.as_ref().is_some_and(desktop_available),
            exited: state.selected.as_ref().is_some_and(|info| info.exited),
            path: state.path.clone(),
        }))
    }
    /// Compatibility consumer for existing platforms. Only changed cells cross FFI.
    pub fn drain_update(&self) -> Result<Option<RenderUpdate>, CoreError> {
        let (_, state) = self.shared()?;
        let mut s = state.lock().map_err(ffi)?;
        take_update(&mut s)
    }
    /// Committed keyboard/IME characters, with no bracketed-paste markers. Special keys
    /// use send_key; send_text remains the explicit paste/command submission API.
    pub fn type_text(&self, text: String) -> Result<(), CoreError> {
        if text.len() > 16000 || text.chars().any(char::is_control) {
            return Err(ffi("typed text too large or contains control characters"));
        }
        self.input(|req| {
            req.input_kind = 0;
            req.input = text.into_bytes();
        })
    }
    pub fn send_text(&self, text: String, submit: bool) -> Result<(), CoreError> {
        if text.len() > 16000 || text.contains('\x1b') {
            return Err(ffi("input too large or contains escape control"));
        }
        self.input(|req| {
            req.input_kind = 1;
            req.text = text;
            req.submit = submit;
        })
    }
    pub fn send_key(&self, key: String) -> Result<(), CoreError> {
        self.input(|req| {
            req.input_kind = 2;
            req.key = key;
        })
    }
    pub fn has_control(&self) -> bool {
        self.shared().ok().is_some_and(|(_, s)| {
            let s = s.lock().unwrap();
            s.controlled && s.connected
        })
    }
    pub fn desktop_attached(&self) -> bool {
        self.shared().ok().is_some_and(|(_, s)| {
            let s = s.lock().unwrap();
            s.selected.as_ref().is_some_and(desktop_available)
        })
    }
    pub fn session_exited(&self) -> bool {
        self.shared().ok().is_some_and(|(_, s)| {
            let s = s.lock().unwrap();
            s.selected.as_ref().is_some_and(|info| info.exited)
        })
    }
    pub fn connection_path(&self) -> String {
        self.shared()
            .map(|(_, s)| s.lock().unwrap().path.clone())
            .unwrap_or_else(|_| "offline".into())
    }
    pub fn use_relay(&self) -> Result<(), CoreError> {
        self.call(Request {
            operation: Operation::Streaming as i32,
            text: "relay-only".into(),
            ..Request::default()
        })?;
        Ok(())
    }
    pub fn create_session(&self, cwd: String) -> Result<RemoteSession, CoreError> {
        let info = self
            .call(Request {
                operation: Operation::Create as i32,
                cwd,
                rows: 24,
                cols: 80,
                ..Request::default()
            })?
            .info
            .ok_or_else(|| ffi("missing session"))?;
        self.call(Request {
            operation: Operation::Detach as i32,
            session: info.id.clone(),
            ..Request::default()
        })?;
        Ok(session(info))
    }
    pub fn read_history(&self) -> Result<Vec<String>, CoreError> {
        let (_, state) = self.shared()?;
        let mut req = state
            .lock()
            .map_err(ffi)?
            .request(Operation::History)
            .map_err(ffi)?;
        req.history_limit = 200;
        let reply = self.call(req)?;
        let mut lines = reply.history;
        if reply.history_truncated {
            lines.insert(0, "[earlier history omitted]".into());
        }
        Ok(lines)
    }
    pub fn read_history_page(
        &self,
        cursor: Option<TerminalHistoryCursor>,
    ) -> Result<TerminalHistoryPage, CoreError> {
        let (_, state) = self.shared()?;
        let (mut request, generation) = {
            let current = state.lock().map_err(ffi)?;
            (
                current.request(Operation::Scrollback).map_err(ffi)?,
                current.generation,
            )
        };
        if let Some(cursor) = cursor {
            if cursor.session != request.session || cursor.epoch != request.session_epoch {
                return Err(ffi("history_session_changed"));
            }
            request.scrollback_id = cursor.view_id;
            request.history_offset = cursor.offset;
        }
        request.history_limit = 200;
        let reply = self.call(request.clone()).map_err(|error| {
            if error.to_string().contains("unknown operation") {
                ffi("Desktop Agent needs an update to read complete history")
            } else {
                error
            }
        })?;
        if reply.snapshot.is_some() {
            return Err(ffi(
                "Desktop Agent needs an update to read complete history",
            ));
        }
        let (_, active) = self.shared()?;
        let current = state.lock().map_err(ffi)?;
        if !Arc::ptr_eq(&active, &state) || current.generation != generation {
            drop(current);
            let _ = self.release_history(TerminalHistoryCursor {
                session: request.session,
                epoch: request.session_epoch,
                view_id: reply.scrollback_id,
                offset: reply.history_next,
            });
            return Err(ffi("history_session_changed"));
        }
        if reply.scrollback_id == 0
            || (request.scrollback_id != 0 && reply.scrollback_id != request.scrollback_id)
            || (reply.history_has_more && reply.history.is_empty())
            || reply.history_next < request.history_offset
            || reply.history_next - request.history_offset != reply.history.len() as u32
            || reply.history_next > reply.history_total
            || reply.history_has_more != (reply.history_next < reply.history_total)
        {
            return Err(ffi("invalid_history_page"));
        }
        Ok(TerminalHistoryPage {
            cursor: TerminalHistoryCursor {
                session: request.session,
                epoch: request.session_epoch,
                view_id: reply.scrollback_id,
                offset: reply.history_next,
            },
            lines: reply.history,
            total: reply.history_total,
            has_more: reply.history_has_more,
            truncated: reply.history_truncated,
        })
    }
    pub fn release_history(&self, cursor: TerminalHistoryCursor) -> Result<(), CoreError> {
        self.call(Request {
            operation: Operation::ReleaseScrollback as i32,
            session: cursor.session,
            session_epoch: cursor.epoch,
            scrollback_id: cursor.view_id,
            ..Request::default()
        })?;
        Ok(())
    }
    /// Styled reading window, independent of the live replica. Offset counts lines above live.
    pub fn read_history_viewport(
        &self,
        cursor: Option<TerminalHistoryCursor>,
        offset: u32,
    ) -> Result<TerminalHistoryViewport, CoreError> {
        let (_, state) = self.shared()?;
        let (mut request, generation) = {
            let current = state.lock().map_err(ffi)?;
            (
                current.request(Operation::Scrollback).map_err(ffi)?,
                current.generation,
            )
        };
        if let Some(cursor) = cursor {
            if cursor.session != request.session || cursor.epoch != request.session_epoch {
                return Err(ffi("history_session_changed"));
            }
            request.scrollback_id = cursor.view_id;
        }
        request.history_offset = offset;
        let reply = self.call(request.clone())?;
        let frame = reply
            .snapshot
            .ok_or_else(|| ffi("missing_history_viewport"))?;
        frame.validate().map_err(ffi)?;
        let cursor = TerminalHistoryCursor {
            session: request.session,
            epoch: request.session_epoch,
            view_id: reply.scrollback_id,
            offset: reply.scrollback_offset,
        };
        let (_, active) = self.shared()?;
        let current = state.lock().map_err(ffi)?;
        if !Arc::ptr_eq(&active, &state) || current.generation != generation {
            drop(current);
            let _ = self.release_history(cursor);
            return Err(ffi("history_session_changed"));
        }
        if frame.epoch != cursor.epoch
            || cursor.view_id == 0
            || (request.scrollback_id != 0 && cursor.view_id != request.scrollback_id)
            || cursor.offset > reply.scrollback_total
        {
            return Err(ffi("invalid_history_viewport"));
        }
        if !history_viewport_is_current(current.replica.state(), &frame) {
            drop(current);
            let _ = self.release_history(cursor);
            return Err(ffi("history_viewport_changed"));
        }
        Ok(TerminalHistoryViewport {
            cursor,
            frame: render_frame(&frame),
            total: reply.scrollback_total,
        })
    }
    pub fn close_selected(&self) -> Result<(), CoreError> {
        let (_, state) = self.shared()?;
        let req = state
            .lock()
            .map_err(ffi)?
            .request(Operation::Close)
            .map_err(ffi)?;
        self.call(req)?;
        let mut s = state.lock().map_err(ffi)?;
        s.selected = None;
        s.controlled = false;
        s.replica.reset();
        Ok(())
    }
}
fn history_viewport_is_current(
    live: Option<&ai_terminal_protocol::Snapshot>,
    history: &ai_terminal_protocol::Snapshot,
) -> bool {
    live.is_none_or(|live| {
        live.revision < history.revision
            || (live.dimensions_epoch == history.dimensions_epoch
                && live.alternate_screen == history.alternate_screen)
    })
}
fn session(s: SessionInfo) -> RemoteSession {
    let desktop_attached = desktop_available(&s);
    RemoteSession {
        id: s.id,
        cwd: s.cwd,
        exited: s.exited,
        exit_code: s.exit_code,
        desktop_attached,
    }
}
fn desktop_available(info: &SessionInfo) -> bool {
    info.availability_epoch == 0 || info.desktop_attached
}
fn take_update(s: &mut State) -> Result<Option<RenderUpdate>, CoreError> {
    if let Some(e) = &s.error {
        return Err(ffi(e));
    }
    if !s.changed {
        return Ok(None);
    }
    let Some(frame) = s.replica.state() else {
        return Ok(None);
    };
    let cursor = frame.cursor.as_ref().unwrap();
    let patches = if s.full {
        frame
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| RenderPatch {
                index: i as u32,
                cell: render_cell(c),
            })
            .collect()
    } else {
        s.dirty
            .iter()
            .map(|&i| RenderPatch {
                index: i as u32,
                cell: render_cell(&frame.cells[i]),
            })
            .collect()
    };
    let value = RenderUpdate {
        generation: s.generation,
        epoch: frame.epoch,
        revision: frame.revision,
        rows: frame.rows,
        cols: frame.cols,
        full: s.full,
        patches,
        cursor_row: cursor.row,
        cursor_col: cursor.col,
        cursor_visible: cursor.visible,
        cursor_shape: cursor.shape,
    };
    s.full = false;
    s.changed = false;
    s.dirty.clear();
    Ok(Some(value))
}
async fn pump(
    mut channel: Channel,
    mut rx: mpsc::Receiver<Job>,
    state: Arc<Mutex<State>>,
) -> Result<()> {
    let mut pending = BTreeMap::<u64, Pending>::new();
    let mut next = 1u64;
    let mut update_id = 0;
    let mut subscription_reply_id = 0;
    let mut updates = BTreeMap::<u64, Reply>::new();
    loop {
        // Input is pipelined; up to 32 in flight remains below the desktop's 64-entry replay window.
        while pending.len() < 32
            && pending
                .keys()
                .next()
                .is_none_or(|oldest| next < oldest + 32)
        {
            let job = match rx.try_recv() {
                Ok(j) => j,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return Ok(()),
            };
            if job.request.operation == Operation::Streaming as i32
                && job.request.text == "relay-only"
            {
                channel.disable_direct();
                if let Some(reply) = job.response {
                    let _ = reply.send(Ok(Reply::default()));
                }
                continue;
            }
            let id = next;
            next = next.checked_add(1).context("sequence exhausted")?;
            channel.stream_request(id, &job.request, false).await?;
            pending.insert(
                id,
                Pending {
                    job,
                    sent: Instant::now(),
                    retried: channel.path() == PathKind::Relay,
                },
            );
        }
        if let Some(event) = channel.stream_next(Duration::from_millis(2)).await? {
            match event {
                StreamEvent::Reply(id, reply) => {
                    if let Some(p) = pending.remove(&id) {
                        if !reply.error.is_empty() {
                            if let Some(response) = p.job.response {
                                let _ = response.send(Err(reply.error.clone()));
                            } else {
                                bail!("input rejected: {}", reply.error)
                            }
                        } else {
                            if matches!(
                                Operation::try_from(p.job.request.operation)?,
                                Operation::Subscribe | Operation::Close
                            ) {
                                if !subscription_boundary(
                                    id,
                                    reply.state_sequence,
                                    &mut subscription_reply_id,
                                    &mut update_id,
                                ) {
                                    if let Some(response) = p.job.response {
                                        let _ = response.send(Ok(reply));
                                    }
                                    continue;
                                }
                                updates.retain(|id, _| *id > update_id);
                            }
                            let mut s = state.lock().unwrap();
                            // Session list/create/history replies must not change the selected terminal.
                            if matches!(
                                Operation::try_from(p.job.request.operation)?,
                                Operation::Subscribe
                                    | Operation::Poll
                                    | Operation::Acquire
                                    | Operation::Input
                                    | Operation::Detach
                            ) {
                                s.apply(&reply)?;
                            }
                            drop(s);
                            if std::env::var_os("AI_TERMINAL_PERF").is_some() {
                                eprintln!(
                                    "terminal_perf request_id={id} queue_us={} ack_us={}",
                                    p.sent.duration_since(p.job.queued).as_micros(),
                                    p.sent.elapsed().as_micros()
                                );
                            }
                            if let Some(response) = p.job.response {
                                let _ = response.send(Ok(reply));
                            }
                        }
                    }
                }
                StreamEvent::Update(id, reply) => {
                    if id <= update_id {
                        channel.state_ack(update_id).await?;
                    } else {
                        ensure!(
                            id <= update_id + 32 && updates.len() < 32,
                            "state sequence gap exceeds recovery window"
                        );
                        if let Some(existing) = updates.get(&id) {
                            ensure!(existing == &reply, "conflicting state retry");
                        }
                        updates.insert(id, reply);
                        ensure!(
                            updates.values().map(Message::encoded_len).sum::<usize>()
                                <= 8 * 1024 * 1024,
                            "state receive budget exceeded"
                        );
                    }
                }
                _ => bail!("unexpected mobile stream event"),
            }
        }
        while let Some(reply) = updates.remove(&(update_id + 1)) {
            if !reply.error.is_empty() {
                bail!("subscription ended: {}", reply.error)
            }
            let result = state.lock().unwrap().apply(&reply);
            if let Err(e) = result {
                if e.downcast_ref::<ai_terminal_protocol::ProtocolError>()
                    == Some(&ai_terminal_protocol::ProtocolError::Baseline)
                {
                    let request = {
                        let s = state.lock().unwrap();
                        s.request(Operation::Subscribe)?
                    };
                    let id = next;
                    next += 1;
                    channel.stream_request(id, &request, false).await?;
                    pending.insert(
                        id,
                        Pending {
                            job: Job {
                                request,
                                response: None,
                                queued: Instant::now(),
                            },
                            sent: Instant::now(),
                            retried: channel.path() == PathKind::Relay,
                        },
                    );
                    updates.clear();
                    break;
                } else {
                    return Err(e);
                }
            }
            update_id += 1;
            channel.state_ack(update_id).await?;
        }
        let path = channel.path();
        let path = if path == PathKind::Direct {
            "direct"
        } else {
            "relay"
        };
        {
            let mut state = state.lock().unwrap();
            if state.path != path {
                state.path = path.into();
            }
        }
        // Retry an entire outstanding window in original ID order so a path switch cannot reorder input.
        if pending
            .values()
            .any(|p| !p.retried && p.sent.elapsed() > Duration::from_millis(500))
        {
            for (&id, p) in pending.iter_mut() {
                channel.stream_request(id, &p.job.request, true).await?;
                p.retried = true;
                p.sent = Instant::now();
            }
        }
        ensure!(
            !pending
                .values()
                .any(|p| p.retried && p.sent.elapsed() > Duration::from_secs(5)),
            "request timed out; outcome unknown, reconnect without replaying"
        );
    }
}

fn subscription_boundary(
    id: u64,
    boundary: u64,
    latest_reply: &mut u64,
    applied: &mut u64,
) -> bool {
    if id <= *latest_reply {
        return false;
    }
    *latest_reply = id;
    // Replies and display messages may cross transport paths. Never roll an applied state
    // cursor back, or wait for already acknowledged updates the desktop no longer retains.
    *applied = (*applied).max(boundary);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_history_viewport_cannot_restore_old_dimensions_or_screen_mode() {
        let history = screen(1, 2, "a");
        let mut live = screen(1, 3, "b");
        assert!(history_viewport_is_current(Some(&live), &history));
        live.dimensions_epoch += 1;
        assert!(!history_viewport_is_current(Some(&live), &history));
        live.dimensions_epoch = history.dimensions_epoch;
        live.alternate_screen = !history.alternate_screen;
        assert!(!history_viewport_is_current(Some(&live), &history));
        live.revision = 1;
        assert!(history_viewport_is_current(Some(&live), &history));
    }
    #[test]
    fn delayed_subscription_replies_never_roll_back_the_applied_cursor() {
        let mut last_reply = 0;
        let mut applied = 5;
        assert!(subscription_boundary(3, 2, &mut last_reply, &mut applied));
        assert_eq!(applied, 5);
        assert!(!subscription_boundary(2, 0, &mut last_reply, &mut applied));
        assert_eq!((last_reply, applied), (3, 5));
        assert!(subscription_boundary(4, 8, &mut last_reply, &mut applied));
        assert_eq!(applied, 8);
    }
    #[test]
    fn older_control_metadata_does_not_discard_a_newer_screen_revision() {
        let mut state = State {
            desired: Some("s".into()),
            ..State::default()
        };
        let first = screen(1, 1, "a");
        state
            .apply(&Reply {
                info: Some(info("s", 1, 1)),
                snapshot: Some(first.clone()),
                ..Reply::default()
            })
            .unwrap();
        state
            .apply(&Reply {
                info: Some(info("s", 1, 2)),
                ..Reply::default()
            })
            .unwrap();
        state.controlled = true;
        let second = screen(1, 2, "b");
        state
            .apply(&Reply {
                info: Some(info("s", 1, 1)),
                delta: Some(second.delta_from(&first).unwrap()),
                ..Reply::default()
            })
            .unwrap();
        assert_eq!(state.replica.state(), Some(&second));
        assert_eq!(state.selected.as_ref().unwrap().control_epoch, 2);
        assert!(state.controlled);
        let third = screen(1, 3, "c");
        state
            .apply(&Reply {
                info: Some(info("s", 1, 2)),
                delta: Some(third.delta_from(&second).unwrap()),
                ..Reply::default()
            })
            .unwrap();
        assert_eq!(state.replica.state(), Some(&third));
    }
    #[test]
    fn desktop_detach_and_reattach_update_input_without_reselecting() {
        let mut state = State {
            desired: Some("s".into()),
            ..State::default()
        };
        let availability = |epoch, attached| SessionInfo {
            id: "s".into(),
            epoch: 1,
            controller: 7,
            control_epoch: 2,
            availability_epoch: epoch,
            desktop_attached: attached,
            ..SessionInfo::default()
        };
        state
            .apply(&Reply {
                info: Some(availability(2, true)),
                snapshot: Some(screen(1, 1, "a")),
                ..Reply::default()
            })
            .unwrap();
        assert!(state.controlled);
        state
            .apply(&Reply {
                info: Some(availability(3, false)),
                ..Reply::default()
            })
            .unwrap();
        assert!(!state.controlled);
        state
            .apply(&Reply {
                info: Some(availability(2, true)),
                ..Reply::default()
            })
            .unwrap();
        assert!(
            !state.controlled,
            "late attachment status restored stale input access"
        );
        state
            .apply(&Reply {
                info: Some(availability(4, true)),
                ..Reply::default()
            })
            .unwrap();
        assert!(state.controlled);
    }
    #[test]
    fn old_agent_without_presence_metadata_keeps_existing_input_behavior() {
        let mut state = State {
            desired: Some("s".into()),
            ..State::default()
        };
        let info = SessionInfo {
            id: "s".into(),
            epoch: 1,
            controller: 7,
            control_epoch: 2,
            ..SessionInfo::default()
        };
        state
            .apply(&Reply {
                info: Some(info.clone()),
                snapshot: Some(screen(1, 1, "a")),
                ..Reply::default()
            })
            .unwrap();
        assert!(desktop_available(&info));
        assert!(state.controlled);
    }
    #[test]
    fn typing_preserves_utf8_without_paste_markers_and_rejects_controls() {
        let (tx, mut rx) = mpsc::channel(8);
        let state = Arc::new(Mutex::new(State {
            selected: Some(info("typing", 10, 20)),
            controlled: true,
            ..State::default()
        }));
        let remote = RemoteTerminal {
            inner: Mutex::new(Some(Worker {
                tx,
                state: state.clone(),
                task: runtime().spawn(std::future::pending()),
            })),
        };
        remote.type_text("git stat中文🙂".into()).unwrap();
        let typed = rx.try_recv().unwrap().request;
        assert_eq!(typed.input_kind, 0);
        assert_eq!(typed.input, "git stat中文🙂".as_bytes());
        assert_eq!(typed.input_seq, 1);
        for invalid in ["\t", "\r", "\n", "\x1b[31m", "\x7f", &"x".repeat(16001)] {
            assert!(remote.type_text(invalid.into()).is_err());
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(state.lock().unwrap().next_input, 2);
        remote.send_key("tab".into()).unwrap();
        let tab = rx.try_recv().unwrap().request;
        assert_eq!(
            (tab.input_kind, tab.key.as_str(), tab.input_seq),
            (2, "tab", 2)
        );
        remote.send_text("paste\tcontent".into(), false).unwrap();
        let pasted = rx.try_recv().unwrap().request;
        assert_eq!(
            (pasted.input_kind, pasted.text.as_str()),
            (1, "paste\tcontent")
        );
        state.lock().unwrap().controlled = false;
        assert!(remote.type_text("rejected".into()).is_err());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn input_queue_backpressure_does_not_consume_sequence_or_change_session_identity() {
        let (tx, mut rx) = mpsc::channel(1);
        let state = Arc::new(Mutex::new(State {
            selected: Some(info("first", 10, 20)),
            controlled: true,
            ..State::default()
        }));
        let remote = RemoteTerminal {
            inner: Mutex::new(Some(Worker {
                tx,
                state: state.clone(),
                task: runtime().spawn(std::future::pending()),
            })),
        };
        remote.send_text("a".into(), false).unwrap();
        assert!(
            remote
                .send_key("enter".into())
                .unwrap_err()
                .to_string()
                .contains("queue full")
        );
        assert_eq!(state.lock().unwrap().next_input, 2);
        let first = rx.try_recv().unwrap().request;
        assert_eq!(
            (
                first.session.as_str(),
                first.session_epoch,
                first.control_epoch,
                first.input_seq
            ),
            ("first", 10, 20, 1)
        );
        {
            let mut state = state.lock().unwrap();
            state.selected = Some(info("second", 30, 40));
            state.next_input = 1;
        }
        remote.send_key("enter".into()).unwrap();
        let second = rx.try_recv().unwrap().request;
        assert_eq!(
            (
                second.session.as_str(),
                second.session_epoch,
                second.control_epoch,
                second.input_seq
            ),
            ("second", 30, 40, 1)
        );
        state.lock().unwrap().controlled = false;
        assert!(remote.send_text("rejected".into(), false).is_err());
        assert!(rx.try_recv().is_err());
        assert_eq!(state.lock().unwrap().next_input, 2);
    }
    #[test]
    fn busy_display_poll_preserves_all_changes_and_coherent_control_status() {
        let mut state = State {
            desired: Some("s".into()),
            ..State::default()
        };
        let first = screen(1, 1, "a");
        state
            .apply(&Reply {
                info: Some(info("s", 1, 1)),
                snapshot: Some(first.clone()),
                ..Reply::default()
            })
            .unwrap();
        assert!(take_update(&mut state).unwrap().unwrap().full);
        let second = screen(1, 2, "b");
        let mut third = second.clone();
        third.revision = 3;
        third.cells[1].text = "c".into();
        third.seal();
        for delta in [
            second.delta_from(&first).unwrap(),
            third.delta_from(&second).unwrap(),
        ] {
            state
                .apply(&Reply {
                    info: Some(info("s", 1, 1)),
                    delta: Some(delta),
                    ..Reply::default()
                })
                .unwrap();
        }
        assert!(state.changed);
        assert_eq!(state.dirty, BTreeSet::from([0, 1]));
        assert_eq!(state.replica.state(), Some(&third));
        let mut invalid = screen(1, 4, "d").delta_from(&third).unwrap();
        invalid.hash = vec![0; 32];
        assert!(
            state
                .apply(&Reply {
                    delta: Some(invalid),
                    ..Reply::default()
                })
                .is_err()
        );
        assert_eq!(state.replica.state(), Some(&third));
        assert_eq!(state.dirty, BTreeSet::from([0, 1]));
        let state = Arc::new(Mutex::new(state));
        let (tx, _rx) = mpsc::channel(1);
        let remote = RemoteTerminal {
            inner: Mutex::new(Some(Worker {
                tx,
                state: state.clone(),
                task: runtime().spawn(std::future::pending()),
            })),
        };
        {
            let held = state.lock().unwrap();
            assert!(remote.poll_display().unwrap().is_none());
            assert!(held.changed);
            assert_eq!(held.dirty, BTreeSet::from([0, 1]));
        }
        let batch = remote.poll_display().unwrap().unwrap();
        assert!(!batch.controlled);
        let update = batch.update.unwrap();
        assert_eq!(update.revision, third.revision);
        assert_eq!(
            update.patches.iter().map(|p| p.index).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(update.patches[0].cell.text, "b");
        assert_eq!(update.patches[1].cell.text, "c");
        assert!(remote.poll_display().unwrap().unwrap().update.is_none());
        state.lock().unwrap().error = Some("offline".into());
        assert!(remote.poll_display().is_err());
    }
    fn screen(epoch: u64, revision: u64, text: &str) -> ai_terminal_protocol::Snapshot {
        let mut s = ai_terminal_protocol::Snapshot {
            version: 1,
            epoch,
            revision,
            rows: 1,
            cols: 2,
            cells: vec![
                ai_terminal_protocol::Cell {
                    text: text.into(),
                    width: 1,
                    ..Default::default()
                },
                ai_terminal_protocol::Cell {
                    text: " ".into(),
                    width: 1,
                    ..Default::default()
                },
            ],
            cursor: Some(Default::default()),
            dimensions_epoch: 1,
            ..Default::default()
        };
        s.seal();
        s
    }
    fn info(id: &str, epoch: u64, control: u64) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            epoch,
            control_epoch: control,
            ..Default::default()
        }
    }
    #[test]
    fn switching_sessions_ignores_late_frames_and_old_control_information() {
        let mut state = State {
            desired: Some("new".into()),
            ..State::default()
        };
        state
            .apply(&Reply {
                info: Some(info("old", 1, 1)),
                snapshot: Some(screen(1, 1, "a")),
                ..Default::default()
            })
            .unwrap();
        assert!(state.replica.state().is_none());
        state
            .apply(&Reply {
                info: Some(info("new", 2, 2)),
                snapshot: Some(screen(2, 1, "b")),
                ..Default::default()
            })
            .unwrap();
        state.controlled = true;
        state
            .apply(&Reply {
                info: Some(info("new", 2, 1)),
                ..Default::default()
            })
            .unwrap();
        assert!(state.controlled);
        assert_eq!(state.selected.unwrap().control_epoch, 2);
    }
    #[test]
    fn incremental_ffi_dirty_cells_are_accumulated_until_display_consumes_them() {
        let mut state = State {
            desired: Some("s".into()),
            ..State::default()
        };
        let first = screen(1, 1, "a");
        let second = screen(1, 2, "b");
        state
            .apply(&Reply {
                info: Some(info("s", 1, 1)),
                snapshot: Some(first.clone()),
                ..Default::default()
            })
            .unwrap();
        state.full = false;
        state.changed = false;
        state
            .apply(&Reply {
                info: Some(info("s", 1, 1)),
                delta: Some(second.delta_from(&first).unwrap()),
                ..Default::default()
            })
            .unwrap();
        assert!(state.changed);
        assert_eq!(state.dirty, BTreeSet::from([0]));
        let mut later = second.clone();
        later.revision = 3;
        later.cursor.as_mut().unwrap().col = 1;
        later.seal();
        state
            .apply(&Reply {
                info: Some(info("s", 1, 1)),
                delta: Some(later.delta_from(&second).unwrap()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(state.dirty, BTreeSet::from([0]));
        assert_eq!(
            state.replica.state().unwrap().cursor.as_ref().unwrap().col,
            1
        );
    }
}
