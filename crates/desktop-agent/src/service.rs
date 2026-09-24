use crate::{
    Control,
    pty::{Output, Session},
    random_id,
};
use ai_terminal_engine::Engine;
use ai_terminal_protocol::{
    Snapshot,
    local::{Operation, Reply, Request, SessionInfo, read_message, write_message},
};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    fs::{self, OpenOptions},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
struct Endpoint {
    address: SocketAddr,
    token: String,
}
pub struct Client {
    endpoint: Endpoint,
    pub id: u64,
    pub(crate) account_scope: String,
    stream: Mutex<Option<TcpStream>>,
}
impl Clone for Client {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            id: self.id,
            account_scope: self.account_scope.clone(),
            stream: Mutex::new(None),
        }
    }
}
impl Client {
    pub fn connect(dir: &Path) -> Result<Self> {
        secure_dir(dir)?;
        let endpoint: Endpoint = serde_json::from_slice(&fs::read(dir.join("endpoint.json"))?)?;
        if !endpoint.address.ip().is_loopback() {
            bail!("local endpoint is not loopback")
        }
        let client = Self {
            endpoint,
            id: random_id(),
            account_scope: String::new(),
            stream: Mutex::new(None),
        };
        client.call(Request {
            operation: Operation::List as i32,
            ..Request::default()
        })?;
        Ok(client)
    }
    pub fn ensure(dir: &Path, executable: &Path) -> Result<Self> {
        if let Ok(client) = Self::connect(dir) {
            return Ok(client);
        }
        secure_dir(dir)?;
        let log = open_private(&dir.join("agent.log"), true)?;
        let mut command = Command::new(executable);
        command
            .arg("--agent")
            .arg("--state-dir")
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x00000008 | 0x00000200);
        }
        let mut child = command.spawn().context("start local Agent")?;
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(client) = Self::connect(dir) {
                return Ok(client);
            }
            if let Some(status) = child.try_wait()? {
                bail!(
                    "Agent startup failed ({status}); see {}",
                    dir.join("agent.log").display()
                )
            }
            if Instant::now() >= until {
                bail!(
                    "Agent startup timed out; see {}",
                    dir.join("agent.log").display()
                )
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn call(&self, mut request: Request) -> Result<Reply> {
        request.token = self.endpoint.token.clone();
        request.client = self.id;
        request.account_scope = self.account_scope.clone();
        let mut guard = self.stream.lock().unwrap();
        if guard.is_none() {
            let stream =
                TcpStream::connect_timeout(&self.endpoint.address, Duration::from_secs(2))?;
            stream.set_nodelay(true)?;
            stream.set_read_timeout(Some(Duration::from_secs(3)))?;
            stream.set_write_timeout(Some(Duration::from_secs(3)))?;
            *guard = Some(stream);
        }
        let result = (|| -> Result<Reply> {
            let stream = guard.as_mut().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(
                if request.operation == Operation::Account as i32 {
                    30
                } else {
                    3
                },
            )))?;
            write_message(stream, &request)?;
            Ok(read_message(stream)?)
        })();
        if result.is_err() {
            *guard = None;
        }
        let reply = result?;
        if !reply.error.is_empty() {
            bail!("{}", reply.error)
        }
        Ok(reply)
    }
}

pub fn default_state_dir() -> PathBuf {
    #[cfg(unix)]
    {
        std::env::temp_dir().join(format!(
            "ai-terminal-{}",
            rustix::process::getuid().as_raw()
        ))
    }
    #[cfg(windows)]
    {
        std::env::temp_dir().join("ai-terminal")
    }
}
pub(crate) fn secure_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(dir)?;
    }
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        bail!("Agent state path must be a real private directory")
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != rustix::process::getuid().as_raw() || meta.mode() & 0o077 != 0 {
            bail!("Agent directory must be owned by this user with permissions 0700")
        }
    }
    Ok(())
}
pub(crate) fn open_private(path: &Path, append: bool) -> Result<fs::File> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && (!meta.is_file() || meta.file_type().is_symlink())
    {
        bail!("invalid Agent state file")
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

type Actor = SyncSender<(Request, SyncSender<Reply>)>;
struct Host {
    account: Arc<crate::account::AccountManager>,
    assistant: crate::assistant::Assistant,
    sessions: Mutex<HashMap<String, Actor>>,
    owners: Mutex<HashMap<String, String>>,
    stop: Arc<AtomicBool>,
    workers: AtomicUsize,
}
pub fn run_agent(dir: &Path) -> Result<()> {
    secure_dir(dir)?;
    let lock = open_private(&dir.join("agent.lock"), false)?;
    lock.try_lock_exclusive()
        .context("another Agent owns this state directory")?;
    #[cfg(unix)]
    {
        rustix::process::setsid().context("detach Agent from controlling terminal")?;
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = Endpoint {
        address: listener.local_addr()?,
        token: format!(
            "{:016x}{:016x}{:016x}{:016x}",
            random_id(),
            random_id(),
            random_id(),
            random_id()
        ),
    };
    let mut endpoint_file = open_private(&dir.join("endpoint.json"), false)?;
    endpoint_file.set_len(0)?;
    serde_json::to_writer(&mut endpoint_file, &endpoint)?;
    endpoint_file.sync_all()?;
    let account = crate::account::AccountManager::new(dir)?;
    let host = Arc::new(Host {
        account: account.clone(),
        assistant: crate::assistant::Assistant::default(),
        sessions: Mutex::new(HashMap::new()),
        owners: Mutex::new(HashMap::new()),
        stop: Arc::new(AtomicBool::new(false)),
        workers: AtomicUsize::new(0),
    });
    crate::remote_bridge::spawn(dir.to_owned(), host.stop.clone());
    account.spawn(host.stop.clone());
    while !host.stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if host.workers.fetch_add(1, Ordering::AcqRel) >= 32 {
                    host.workers.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let host = host.clone();
                let token = endpoint.token.clone();
                thread::spawn(move || {
                    // macOS accepts inherit the listener's nonblocking flag.
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_nodelay(true);
                    while !host.stop.load(Ordering::Acquire) {
                        let request = match read_message::<_, Request>(&mut stream) {
                            Ok(req) => req,
                            Err(_) => break,
                        };
                        if request.token != token {
                            let _ = write_message(&mut stream, &error("unauthorized local client"));
                            break;
                        }
                        // An authenticated keyboard connection can be idle indefinitely.
                        let _ = stream.set_read_timeout(None);
                        let reply =
                            dispatch(&host, request).unwrap_or_else(|e| error(e.to_string()));
                        if write_message(&mut stream, &reply).is_err() {
                            break;
                        }
                    }
                    host.workers.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(e.into()),
        }
    }
    let actors = std::mem::take(&mut *host.sessions.lock().unwrap());
    for (_, actor) in actors {
        let _ = request_actor(
            &actor,
            Request {
                operation: Operation::Close as i32,
                ..Request::default()
            },
        );
    }
    let _ = fs::remove_file(dir.join("endpoint.json"));
    drop(lock);
    Ok(())
}
fn error(message: impl Into<String>) -> Reply {
    Reply {
        error: message.into(),
        ..Reply::default()
    }
}
fn request_actor(actor: &Actor, request: Request) -> Result<Reply> {
    let (tx, rx) = mpsc::sync_channel(1);
    actor
        .try_send((request, tx))
        .context("session busy or closed")?;
    rx.recv_timeout(Duration::from_secs(2))
        .context("session did not respond")
}
fn dispatch(host: &Host, request: Request) -> Result<Reply> {
    let op = Operation::try_from(request.operation).context("unknown operation")?;
    if request.client == 0 {
        bail!("invalid client")
    }
    if request.input.len() > 65536
        || request.text.len() > 16000
        || request.key.len() > 32
        || request.command.len() > 128
        || request.cwd.len() > 16384
    {
        bail!("request exceeds session limits")
    }
    if !request.account_scope.is_empty() {
        if request.account_scope != host.account.owner() {
            bail!("account changed; reconnect")
        }
        if !request.session.is_empty()
            && host.owners.lock().unwrap().get(&request.session) != Some(&request.account_scope)
        {
            bail!("session belongs to another account")
        }
    }
    match op {
        Operation::Assistant => {
            let message = crate::assistant::Request::parse(&request.text)?;
            let actor = host
                .sessions
                .lock()
                .unwrap()
                .get(&request.session)
                .cloned()
                .context("session not found")?;
            let read_actor = actor.clone();
            let read_request = request.clone();
            let write_request = request.clone();
            let reader_account = host.account.clone();
            let writer_account = host.account.clone();
            let terminal = crate::assistant::TerminalAccess {
                observe: Box::new(move || {
                    if !read_request.account_scope.is_empty()
                        && reader_account.owner() != read_request.account_scope
                    {
                        bail!("account changed; assistant stopped");
                    }
                    let reply = request_actor(
                        &read_actor,
                        Request {
                            operation: Operation::Poll as i32,
                            revision: 0,
                            ..read_request.clone()
                        },
                    )?;
                    if !reply.error.is_empty() {
                        bail!("{}", reply.error);
                    }
                    Ok(crate::assistant::Observation {
                        screen: reply.snapshot.context("terminal snapshot unavailable")?,
                        info: reply.info.context("terminal session unavailable")?,
                    })
                }),
                input: Box::new(move |input| {
                    if !write_request.account_scope.is_empty()
                        && writer_account.owner() != write_request.account_scope
                    {
                        bail!("account changed; assistant input cancelled");
                    }
                    let reply = request_actor(
                        &actor,
                        Request {
                            operation: Operation::AssistantInput as i32,
                            input_kind: 1,
                            text: input.text,
                            submit: input.submit,
                            ..write_request.clone()
                        },
                    )?;
                    if !reply.error.is_empty() {
                        bail!("{}", reply.error);
                    }
                    Ok(())
                }),
            };
            let result =
                host.assistant
                    .call(&request.account_scope, &request.session, message, terminal)?;
            Ok(Reply {
                history: vec![result],
                ..Reply::default()
            })
        }
        Operation::Account => {
            let result = host.account.call(&request.text)?;
            let owner = host.account.owner();
            if !owner.is_empty() {
                let sessions = host.sessions.lock().unwrap();
                let mut owners = host.owners.lock().unwrap();
                for id in sessions.keys() {
                    let value = owners.entry(id.clone()).or_default();
                    if value.is_empty() {
                        *value = owner.clone();
                    }
                }
            }
            Ok(Reply {
                history: vec![result],
                ..Reply::default()
            })
        }
        Operation::List => {
            let sessions: Vec<_> = host.sessions.lock().unwrap().values().cloned().collect();
            let mut reply = Reply::default();
            for actor in sessions {
                if let Ok(r) = request_actor(
                    &actor,
                    Request {
                        operation: Operation::Poll as i32,
                        revision: u64::MAX,
                        ..Request::default()
                    },
                ) && let Some(info) = r.info
                    && (request.account_scope.is_empty()
                        || host.owners.lock().unwrap().get(&info.id)
                            == Some(&request.account_scope))
                {
                    reply.sessions.push(info);
                }
            }
            reply.sessions.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(reply)
        }
        Operation::Create => {
            let mut sessions = host.sessions.lock().unwrap();
            if sessions.len() >= 16 {
                bail!("16-session limit reached; close unused sessions")
            }
            let id = format!("{:016x}", random_id());
            let epoch = random_id();
            let rows = u16::try_from(request.rows)?;
            let cols = u16::try_from(request.cols)?;
            let engine = Engine::new(rows, cols, epoch)?;
            let command: Vec<OsString> = request.command.iter().map(OsString::from).collect();
            let cwd = if request.cwd.is_empty() {
                std::env::current_dir()?
            } else {
                PathBuf::from(&request.cwd)
            };
            let pty = Session::spawn(&command, Some(&cwd), rows, cols)?;
            let (tx, rx) = mpsc::sync_channel(128);
            let actor_id = id.clone();
            let client = request.client;
            thread::Builder::new()
                .name(format!("session-{id}"))
                .spawn(move || session_loop(actor_id, cwd, engine, pty, client, rx))?;
            host.owners.lock().unwrap().insert(
                id.clone(),
                if request.account_scope.is_empty() {
                    host.account.owner()
                } else {
                    // A remote create accepted before an account switch still belongs to its
                    // authenticated caller, never to the account active after PTY startup.
                    request.account_scope.clone()
                },
            );
            sessions.insert(id, tx.clone());
            drop(sessions);
            request_actor(
                &tx,
                Request {
                    operation: Operation::Poll as i32,
                    ..Request::default()
                },
            )
        }
        Operation::Shutdown => {
            host.stop.store(true, Ordering::Release);
            Ok(Reply::default())
        }
        _ => {
            let actor = host
                .sessions
                .lock()
                .unwrap()
                .get(&request.session)
                .cloned()
                .context("session not found")?;
            let reply = request_actor(&actor, request.clone())?;
            if op == Operation::Close && reply.error.is_empty() {
                host.sessions.lock().unwrap().remove(&request.session);
                host.owners.lock().unwrap().remove(&request.session);
            }
            Ok(reply)
        }
    }
}

fn session_loop(
    id: String,
    cwd: PathBuf,
    mut engine: Engine,
    mut pty: Session,
    client: u64,
    rx: Receiver<(Request, SyncSender<Reply>)>,
) {
    let mut control = Control::default();
    control.acquire(client).unwrap();
    pty.set_fence(control.epoch);
    let mut info = SessionInfo {
        id,
        epoch: engine.snapshot().epoch,
        cwd: cwd.to_string_lossy().into_owned(),
        ..SessionInfo::default()
    };
    let mut snapshots = VecDeque::from([engine.snapshot()]);
    let mut eof = false;
    let mut last_publish = Instant::now();
    let mut watchers: Vec<(Request, SyncSender<Reply>, Instant)> = Vec::new();
    loop {
        let start = Instant::now();
        while !eof && start.elapsed() < Duration::from_millis(2) {
            match pty.output.try_recv() {
                Ok(Output::Bytes(bytes)) => {
                    for response in engine.feed(&bytes) {
                        if let Err(e) = pty.write(response) {
                            info.error = e.to_string();
                        }
                    }
                }
                Ok(Output::End) | Err(mpsc::TryRecvError::Disconnected) => {
                    engine.finish();
                    eof = true;
                }
                Ok(Output::Error(e)) => {
                    info.error = e;
                    eof = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        for response in engine.tick() {
            if let Err(e) = pty.write(response) {
                info.error = e.to_string();
            }
        }
        if let Err(e) = pty.check_writer() {
            info.error = e.to_string();
        }
        if eof && let Ok(Some(status)) = pty.exit_status() {
            info.exited = true;
            info.exit_code = status.exit_code();
        }
        if engine.snapshot_revision() != snapshots.back().unwrap().revision
            && (eof || last_publish.elapsed() >= Duration::from_millis(4))
        {
            let s = engine.snapshot();
            if let Err(e) = s.validate() {
                info.error = e.to_string();
            } else {
                snapshots.push_back(s);
                if snapshots.len() > 16 {
                    snapshots.pop_front();
                }
            }
            last_publish = Instant::now();
        }
        let mut index = 0;
        while index < watchers.len() {
            let (req, _, deadline) = &watchers[index];
            if req.revision != snapshots.back().unwrap().revision
                || req.control_epoch != control.epoch
                || info.exited
                || Instant::now() >= *deadline
            {
                let (mut req, tx, _) = watchers.swap_remove(index);
                req.operation = Operation::Poll as i32;
                let result = handle_session(
                    req,
                    &mut engine,
                    &pty,
                    &mut control,
                    &mut info,
                    &mut snapshots,
                );
                let _ = tx.send(result.unwrap_or_else(|e| error(e.to_string())));
            } else {
                index += 1;
            }
        }
        match rx.recv_timeout(Duration::from_millis(1)) {
            Ok((mut req, reply_tx)) => {
                if req.operation == Operation::Watch as i32 {
                    if watchers.len() >= 16 {
                        let _ = reply_tx.send(error("too many subscribers"));
                        continue;
                    }
                    if req.session_epoch == info.epoch
                        && req.revision == snapshots.back().unwrap().revision
                        && req.control_epoch == control.epoch
                        && !info.exited
                    {
                        watchers.push((req, reply_tx, Instant::now() + Duration::from_secs(1)));
                        continue;
                    }
                    req.operation = Operation::Poll as i32;
                }
                let close = req.operation == Operation::Close as i32;
                let result = handle_session(
                    req,
                    &mut engine,
                    &pty,
                    &mut control,
                    &mut info,
                    &mut snapshots,
                );
                if close && let Ok(reply) = &result {
                    drop(pty);
                    let _ = reply_tx.send(reply.clone());
                    return;
                }
                let _ = reply_tx.send(result.unwrap_or_else(|e| error(e.to_string())));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
fn handle_session(
    req: Request,
    engine: &mut Engine,
    pty: &Session,
    control: &mut Control,
    info: &mut SessionInfo,
    snapshots: &mut VecDeque<Snapshot>,
) -> Result<Reply> {
    let op = Operation::try_from(req.operation)?;
    if req.session_epoch != 0 && req.session_epoch != info.epoch {
        bail!("stale session epoch")
    }
    let mut reply = Reply::default();
    match op {
        Operation::Acquire => {
            if info.exited {
                bail!("session exited")
            }
            control.acquire(req.client)?;
            pty.set_fence(control.epoch);
        }
        Operation::Detach => {
            control.release(req.client);
            pty.set_fence(control.epoch);
        }
        Operation::AssistantInput => {
            if info.exited {
                bail!("session exited");
            }
            control.check(req.client, req.control_epoch)?;
            if req.input_seq != control.next {
                bail!("manual input occurred; stale AI input cancelled");
            }
            let bytes = encode_input(&req, engine)?;
            pty.write_controlled(control.epoch, bytes)?;
        }
        Operation::Input => {
            let started = Instant::now();
            if info.exited {
                bail!("session exited")
            }
            let signature = req.encode_to_vec();
            if !control.input(req.client, req.control_epoch, req.input_seq, &signature)? {
                let bytes = encode_input(&req, engine)?;
                pty.write_controlled(control.epoch, bytes)?;
                control.commit(req.input_seq, signature);
            }
            reply.accepted_input_seq = req.input_seq;
            if std::env::var_os("AI_TERMINAL_PERF").is_some() {
                eprintln!(
                    "terminal_perf input_seq={} pty_enqueue_us={}",
                    req.input_seq,
                    started.elapsed().as_micros()
                );
            }
        }
        Operation::Resize => {
            control.check(req.client, req.control_epoch)?;
            let rows = u16::try_from(req.rows)?;
            let cols = u16::try_from(req.cols)?;
            ai_terminal_engine::check_size(rows, cols)?;
            pty.resize(rows, cols)?;
            engine.resize(rows, cols)?;
            snapshots.push_back(engine.snapshot());
            if snapshots.len() > 16 {
                snapshots.pop_front();
            }
        }
        Operation::History => {
            // Point-in-time read, not a independently reflowed interactive viewport.
            let (lines, truncated) =
                engine.history(req.history_offset as usize, req.history_limit as usize);
            reply.history = lines;
            reply.history_truncated = truncated;
        }
        Operation::Close => {}
        Operation::Poll => {
            let current = snapshots.back().unwrap();
            if req.revision != u64::MAX && req.revision != current.revision {
                if let Some(base) = snapshots.iter().find(|s| s.revision == req.revision)
                    && let Some(delta) = current.delta_from(base)
                    && delta.encoded_len() < current.encoded_len()
                {
                    reply.delta = Some(delta)
                } else {
                    reply.snapshot = Some(current.clone())
                }
            }
        }
        _ => bail!("invalid session operation"),
    }
    info.controller = control.owner;
    info.control_epoch = control.epoch;
    info.next_input_seq = control.next;
    reply.info = Some(info.clone());
    Ok(reply)
}

fn encode_input(request: &Request, engine: &Engine) -> Result<Vec<u8>> {
    match request.input_kind {
        0 => Ok(request.input.clone()),
        1 => {
            if request.text.contains('\x1b') {
                bail!("paste cannot contain escape control")
            }
            let paste = engine.bracketed_paste();
            if !paste
                && !request.submit
                && (request.text.contains('\n') || request.text.contains('\r'))
            {
                bail!("active program does not support safe multiline paste")
            }
            let mut bytes = if paste {
                format!("\x1b[200~{}\x1b[201~", request.text).into_bytes()
            } else {
                request
                    .text
                    .replace("\r\n", "\r")
                    .replace('\n', "\r")
                    .into_bytes()
            };
            if request.submit {
                bytes.push(13)
            }
            Ok(bytes)
        }
        2 => Ok(match request.key.as_str() {
            "enter" => vec![13],
            "ctrl_c" => vec![3],
            "ctrl_d" => vec![4],
            "tab" => vec![9],
            "escape" => vec![27],
            "backspace" => vec![127],
            "up" | "down" | "right" | "left" => {
                let suffix = match request.key.as_str() {
                    "up" => 'A',
                    "down" => 'B',
                    "right" => 'C',
                    _ => 'D',
                };
                format!(
                    "\x1b{}{suffix}",
                    if engine.application_cursor() {
                        'O'
                    } else {
                        '['
                    }
                )
                .into_bytes()
            }
            _ => bail!("unsupported key"),
        }),
        _ => bail!("unknown input kind"),
    }
}
