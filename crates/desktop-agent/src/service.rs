mod authorization;
mod runtime;
mod scrollback;
use crate::{
    Control,
    pty::{Output, Session},
    random_id,
};
use ai_terminal_engine::Engine;
use ai_terminal_protocol::{
    ProtocolError, Snapshot,
    local::{
        Operation, Reply, Request, SESSION_CLOSED_ERROR, SessionInfo, read_message, write_message,
    },
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
    pub(crate) device_scope: String,
    stream: Mutex<Option<TcpStream>>,
}
impl Clone for Client {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            id: self.id,
            account_scope: self.account_scope.clone(),
            device_scope: self.device_scope.clone(),
            stream: Mutex::new(None),
        }
    }
}
impl Client {
    pub fn connect(dir: &Path) -> Result<Self> {
        secure_dir(dir)?;
        let endpoint_path = if dir.join("runtime/endpoint.json").exists() {
            dir.join("runtime/endpoint.json")
        } else {
            dir.join("endpoint.json")
        };
        let endpoint: Endpoint = serde_json::from_slice(&fs::read(endpoint_path)?)?;
        if !endpoint.address.ip().is_loopback() {
            bail!("local endpoint is not loopback")
        }
        let client = Self {
            endpoint,
            id: random_id(),
            account_scope: String::new(),
            device_scope: String::new(),
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
        secure_dir(&dir.join("logs"))?;
        let log_path = dir.join("logs/agent.log");
        if fs::metadata(&log_path).is_ok_and(|m| m.len() > 8 * 1024 * 1024) {
            let backup = dir.join("logs/agent.previous.log");
            if backup.exists() {
                fs::remove_file(&backup)?;
            }
            fs::rename(&log_path, backup)?;
        }
        let log = open_private(&log_path, true)?;
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
                    dir.join("logs/agent.log").display()
                )
            }
            if Instant::now() >= until {
                bail!(
                    "Agent startup timed out; see {}",
                    dir.join("logs/agent.log").display()
                )
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn call(&self, mut request: Request) -> Result<Reply> {
        request.token = self.endpoint.token.clone();
        request.client = self.id;
        request.account_scope = self.account_scope.clone();
        request.device_scope = self.device_scope.clone();
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
                if request.operation == Operation::Account as i32
                    || request.operation == Operation::Configuration as i32
                    || request.operation == Operation::Agent as i32
                {
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

pub fn default_state_dir() -> Result<PathBuf> {
    crate::state::default_root()
}
pub(crate) fn secure_dir(dir: &Path) -> Result<()> {
    #[cfg(windows)]
    let created = !dir.exists();
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
    #[cfg(windows)]
    crate::private_acl::protect(dir, created, true)?;
    Ok(())
}
pub(crate) fn open_private(path: &Path, append: bool) -> Result<fs::File> {
    #[cfg(windows)]
    let created = !path.exists();
    if let Ok(meta) = fs::symlink_metadata(path)
        && (!meta.is_file() || meta.file_type().is_symlink())
    {
        bail!("invalid Agent state file")
    }
    #[cfg(unix)]
    if let Ok(meta) = fs::symlink_metadata(path) {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            meta.uid() == rustix::process::getuid().as_raw() && meta.mode() & 0o077 == 0,
            "Agent state file must be owned by this user with permissions 0600"
        );
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(windows)]
    crate::private_acl::protect(path, created, false)?;
    Ok(file)
}

type ExecutionGate = Arc<Mutex<bool>>;
struct ActorMessage {
    request: Request,
    reply: SyncSender<Reply>,
    gate: Option<ExecutionGate>,
    authorization: Option<Arc<ai_terminal_agent_runtime::host::AuthorizationPermit>>,
}
type Actor = SyncSender<ActorMessage>;
const DESKTOP_LEASE: Duration = Duration::from_secs(15);
struct DesktopPresence {
    clients: HashMap<u64, Instant>,
    epoch: u64,
}
impl Default for DesktopPresence {
    fn default() -> Self {
        Self {
            clients: HashMap::new(),
            epoch: 1,
        }
    }
}
impl DesktopPresence {
    fn active(&self) -> bool {
        !self.clients.is_empty()
    }
    fn attach(&mut self, client: u64) -> Result<()> {
        if client == 0 {
            bail!("invalid Desktop client")
        }
        let was_active = self.active();
        self.clients.insert(client, Instant::now());
        if !was_active {
            self.epoch += 1;
        }
        Ok(())
    }
    fn detach(&mut self, client: u64) {
        let was_active = self.active();
        self.clients.remove(&client);
        if was_active && !self.active() {
            self.epoch += 1;
        }
    }
    fn touch(&mut self, client: u64) {
        if let Some(last) = self.clients.get_mut(&client) {
            *last = Instant::now();
        }
    }
    fn expire(&mut self) -> bool {
        let was_active = self.active();
        self.clients
            .retain(|_, last| last.elapsed() < DESKTOP_LEASE);
        if was_active && !self.active() {
            self.epoch += 1;
            return true;
        }
        false
    }
}
struct Host {
    agents: Arc<ai_terminal_agent_runtime::host::AgentHost>,
    state_dir: PathBuf,
    account: Arc<crate::account::AccountManager>,
    config: crate::config::ConfigService,
    assistant: crate::assistant::Assistant,
    sessions: Mutex<HashMap<String, Actor>>,
    session_order: Mutex<Vec<String>>,
    recent_directories: Mutex<crate::recent_directories::RecentDirectories>,
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
    secure_dir(&dir.join("runtime"))?;
    if !dir.join("runtime/agent.lock").exists() {
        fs::hard_link(dir.join("agent.lock"), dir.join("runtime/agent.lock"))?;
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
    let mut endpoint_file = open_private(&dir.join("runtime/endpoint.json"), false)?;
    endpoint_file.set_len(0)?;
    serde_json::to_writer(&mut endpoint_file, &endpoint)?;
    endpoint_file.sync_all()?;
    if std::env::var_os("AI_TERMINAL_AI_BASE_URL").is_some()
        && std::env::var("AI_TERMINAL_LEGACY_ASSISTANT").as_deref() != Ok("1")
    {
        eprintln!(
            "Legacy AI environment is not applied. Import explicitly with: aTerminal config import-legacy-env"
        );
    }
    let account = crate::account::AccountManager::new(dir)?;
    let async_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .build()?;
    let store = Arc::new(ai_terminal_agent_runtime::store::Store::open(
        &dir.join("data/agent.sqlite3"),
    )?);
    let agents =
        ai_terminal_agent_runtime::host::AgentHost::new(store, async_runtime.handle().clone());
    let host = Arc::new(Host {
        agents,
        state_dir: dir.into(),
        recent_directories: Mutex::new(crate::recent_directories::RecentDirectories::new(dir)),
        account: account.clone(),
        config: crate::config::ConfigService::open(dir)?,
        assistant: crate::assistant::Assistant::default(),
        sessions: Mutex::new(HashMap::new()),
        session_order: Mutex::new(Vec::new()),
        owners: Mutex::new(HashMap::new()),
        stop: Arc::new(AtomicBool::new(false)),
        workers: AtomicUsize::new(0),
    });
    crate::remote_bridge::spawn(dir.to_owned(), host.stop.clone());
    account.spawn(host.stop.clone());
    runtime::spawn_recorder(Arc::downgrade(&host));
    spawn_directory_recorder(Arc::downgrade(&host));
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
    host.agents.cancel_all();
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
    let _ = fs::remove_file(dir.join("runtime/endpoint.json"));
    async_runtime.shutdown_timeout(Duration::from_secs(2));
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
    request_actor_guarded(actor, request, None)
}
fn request_actor_guarded(
    actor: &Actor,
    request: Request,
    gate: Option<ExecutionGate>,
) -> Result<Reply> {
    request_actor_authorized(actor, request, gate, None)
}
fn request_actor_authorized(
    actor: &Actor,
    request: Request,
    gate: Option<ExecutionGate>,
    authorization: Option<Arc<ai_terminal_agent_runtime::host::AuthorizationPermit>>,
) -> Result<Reply> {
    let (tx, rx) = mpsc::sync_channel(1);
    match actor.try_send(ActorMessage {
        request,
        reply: tx,
        gate,
        authorization,
    }) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Disconnected(_)) => bail!(SESSION_CLOSED_ERROR),
        Err(mpsc::TrySendError::Full(_)) => bail!("session busy"),
    }
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(reply) => Ok(reply),
        Err(mpsc::RecvTimeoutError::Disconnected) => bail!(SESSION_CLOSED_ERROR),
        Err(mpsc::RecvTimeoutError::Timeout) => bail!("session did not respond"),
    }
}
fn dispatch(host: &Arc<Host>, request: Request) -> Result<Reply> {
    let op = Operation::try_from(request.operation).context("unknown operation")?;
    if request.client == 0 {
        bail!("invalid client")
    }
    let text_limit = match op {
        Operation::Configuration => 1024 * 1024,
        Operation::Agent => 65536,
        _ => 16000,
    };
    if request.input.len() > 65536
        || request.text.len() > text_limit
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
        if op != Operation::Agent
            && !request.session.is_empty()
            && host.owners.lock().unwrap().get(&request.session) != Some(&request.account_scope)
        {
            bail!("session belongs to another account")
        }
    }
    match op {
        Operation::RemoteScreens | Operation::RemoteScreenFrame => {
            crate::screens::dispatch(request)
        }
        Operation::Agent => runtime::dispatch(host, request),
        Operation::Configuration => {
            let command: crate::config::Command = serde_json::from_str(&request.text)
                .map_err(|_| anyhow::anyhow!("invalid_configuration_request"))?;
            // Disk reload is a local management operation; remote clients submit
            // a complete scoped candidate with an expected revision.
            if !request.account_scope.is_empty()
                && matches!(command, crate::config::Command::Reload)
            {
                bail!("remote configuration cannot reload local files");
            }
            let view = host.config.execute(&host.account.owner(), command)?;
            Ok(Reply {
                history: vec![serde_json::to_string(&view)?],
                ..Reply::default()
            })
        }
        Operation::Assistant => {
            anyhow::ensure!(
                std::env::var("AI_TERMINAL_LEGACY_ASSISTANT").as_deref() == Ok("1"),
                "legacy_assistant_disabled_use_agent_v1"
            );
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
            // Session IDs are random, so preserve creation order explicitly. Mobile clients
            // select the first live session when opening a workspace.
            let sessions: Vec<_> = {
                let sessions = host.sessions.lock().unwrap();
                host.session_order
                    .lock()
                    .unwrap()
                    .iter()
                    .rev()
                    .filter_map(|id| sessions.get(id).cloned())
                    .collect()
            };
            let mut reply = Reply {
                screen_protocol_version: ai_terminal_protocol::screens::SCREEN_PROTOCOL_VERSION,
                ..Default::default()
            };
            for actor in sessions {
                if let Ok(r) = request_actor(
                    &actor,
                    Request {
                        operation: Operation::Poll as i32,
                        revision: u64::MAX,
                        client: request.client,
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
            let owner = if request.account_scope.is_empty() {
                host.account.owner()
            } else {
                request.account_scope.clone()
            };
            reply.recent_directories = host
                .recent_directories
                .lock()
                .unwrap()
                .list(&owner)
                .unwrap_or_default();
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
            let cwd = crate::recent_directories::validate(&cwd)?;
            // A create accepted before an account switch stays with its authenticated caller.
            let owner = if request.account_scope.is_empty() {
                host.account.owner()
            } else {
                request.account_scope.clone()
            };
            let recorded_cwd = cwd.clone();
            let pty = Session::spawn_integrated(
                &command,
                Some(&cwd),
                rows,
                cols,
                request
                    .shell_integration
                    .then_some(host.state_dir.as_path()),
            )?;
            let (tx, rx) = mpsc::sync_channel(128);
            let actor_id = id.clone();
            let client = request.client;
            thread::Builder::new()
                .name(format!("session-{id}"))
                .spawn(move || session_loop(actor_id, cwd, engine, pty, client, rx))?;
            host.owners
                .lock()
                .unwrap()
                .insert(id.clone(), owner.clone());
            if let Err(error) = host
                .recent_directories
                .lock()
                .unwrap()
                .record(&owner, &recorded_cwd)
            {
                eprintln!("Unable to save recent working directory: {error}");
            }
            host.session_order.lock().unwrap().push(id.clone());
            sessions.insert(id, tx.clone());
            drop(sessions);
            request_actor(
                &tx,
                Request {
                    operation: Operation::Poll as i32,
                    client,
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
                .ok_or_else(|| anyhow::anyhow!(SESSION_CLOSED_ERROR))?;
            let reply = request_actor(&actor, request.clone())?;
            if matches!(op, Operation::Input | Operation::Resize)
                && reply.error.is_empty()
                && let Some(info) = &reply.info
            {
                let owner = host
                    .owners
                    .lock()
                    .unwrap()
                    .get(&request.session)
                    .cloned()
                    .unwrap_or_default();
                host.agents
                    .preempt(&owner, &request.session, info.manual_revision);
            }
            if matches!(op, Operation::Close | Operation::AgentClose) && reply.error.is_empty() {
                host.sessions.lock().unwrap().remove(&request.session);
                host.session_order
                    .lock()
                    .unwrap()
                    .retain(|id| id != &request.session);
                host.owners.lock().unwrap().remove(&request.session);
            }
            Ok(reply)
        }
    }
}

/// Observe outside session actors so OS queries and disk writes cannot stall terminal input.
fn spawn_directory_recorder(host: std::sync::Weak<Host>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut previous: HashMap<String, (String, PathBuf)> = HashMap::new();
        loop {
            thread::sleep(Duration::from_secs(2));
            let Some(host) = host.upgrade() else { break };
            if host.stop.load(Ordering::Acquire) {
                break;
            }
            let ids = host.session_order.lock().unwrap().clone();
            previous.retain(|id, _| ids.contains(id));
            for id in ids {
                let actor = host.sessions.lock().unwrap().get(&id).cloned();
                let owner = host.owners.lock().unwrap().get(&id).cloned();
                let (Some(actor), Some(owner)) = (actor, owner) else {
                    continue;
                };
                let Ok(reply) = request_actor(
                    &actor,
                    Request {
                        operation: Operation::Poll as i32,
                        revision: u64::MAX,
                        ..Request::default()
                    },
                ) else {
                    continue;
                };
                let Some(info) = reply.info else { continue };
                let Some(cwd) = crate::process::cwd(&info)
                    .and_then(|p| crate::recent_directories::validate(&p).ok())
                else {
                    continue;
                };
                // Creation already recorded the initial path. Idle sessions must not reorder MRU.
                let last = previous
                    .entry(id)
                    .or_insert_with(|| (owner.clone(), PathBuf::from(&info.cwd)));
                if last == &(owner.clone(), cwd.clone()) {
                    continue;
                }
                if host
                    .recent_directories
                    .lock()
                    .unwrap()
                    .record(&owner, &cwd)
                    .is_ok()
                {
                    *last = (owner, cwd);
                }
            }
        }
    })
}

fn session_loop(
    id: String,
    cwd: PathBuf,
    mut engine: Engine,
    mut pty: Session,
    client: u64,
    rx: Receiver<ActorMessage>,
) {
    let mut control = Control::default();
    control.acquire(client).unwrap();
    let mut presence = DesktopPresence::default();
    let mut info = SessionInfo {
        id,
        epoch: engine.snapshot().epoch,
        cwd: cwd.to_string_lossy().into_owned(),
        availability_epoch: presence.epoch,
        ..SessionInfo::default()
    };
    authorization::initialize(&mut info);
    let mut snapshots = VecDeque::from([engine.snapshot()]);
    let mut scrollback = scrollback::Views::default();
    let mut eof = false;
    let mut shell_poll = Instant::now() - Duration::from_secs(1);
    let mut last_publish = Instant::now();
    let mut warned_invalid_frame = false;
    let mut watchers: Vec<(Request, SyncSender<Reply>, Instant)> = Vec::new();
    loop {
        scrollback.expire(&engine);
        if presence.expire() {
            info.desktop_attached = false;
            info.availability_epoch = presence.epoch;
        }
        if shell_poll.elapsed() >= Duration::from_millis(250) {
            authorization::observe(&mut info, pty.shell_observation());
            shell_poll = Instant::now();
        }
        info.process_id = pty.process_id().unwrap_or(0);
        info.process_identity = pty.process_identity.clone();
        info.foreground_group = pty.foreground_group().unwrap_or(0);
        let start = Instant::now();
        while !eof && start.elapsed() < Duration::from_millis(2) {
            match pty.output.try_recv() {
                Ok(Output::Bytes(bytes)) => {
                    for response in engine.feed(&bytes) {
                        if let Err(e) = pty.write(response) {
                            info.error = e.to_string();
                        } else {
                            authorization::input(&mut info, None);
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
        if let Ok(Some(status)) = pty.exit_status() {
            info.exited = true;
            info.exit_code = status.exit_code();
        }
        if engine.snapshot_revision() != snapshots.back().unwrap().revision
            && (eof || last_publish.elapsed() >= Duration::from_millis(4))
        {
            let s = engine.snapshot();
            let revision = s.revision;
            if let Err(e) = publish_snapshot(s, &mut snapshots) {
                if !warned_invalid_frame {
                    eprintln!(
                        "session {}: skipped invalid display revision {revision}: {e}",
                        info.id
                    );
                }
                warned_invalid_frame = true;
            } else {
                warned_invalid_frame = false;
            }
            last_publish = Instant::now();
        }
        let mut index = 0;
        while index < watchers.len() {
            let (req, _, deadline) = &watchers[index];
            if req.revision != snapshots.back().unwrap().revision
                || req.control_epoch != control.metadata(req.client).1
                || req.availability_epoch != info.availability_epoch
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
                    &mut presence,
                    &mut info,
                    &mut snapshots,
                    &mut scrollback,
                    None,
                    None,
                );
                let _ = tx.send(result.unwrap_or_else(|e| error(e.to_string())));
            } else {
                index += 1;
            }
        }
        match rx.recv_timeout(Duration::from_millis(1)) {
            Ok(ActorMessage {
                request: mut req,
                reply: reply_tx,
                gate,
                authorization,
            }) => {
                if req.operation == Operation::Watch as i32 {
                    if watchers.len() >= 16 {
                        let _ = reply_tx.send(error("too many subscribers"));
                        continue;
                    }
                    if req.session_epoch == info.epoch
                        && req.revision == snapshots.back().unwrap().revision
                        && req.control_epoch == control.metadata(req.client).1
                        && req.availability_epoch == info.availability_epoch
                        && !info.exited
                    {
                        watchers.push((req, reply_tx, Instant::now() + Duration::from_secs(1)));
                        continue;
                    }
                    req.operation = Operation::Poll as i32;
                }
                let close = req.operation == Operation::Close as i32
                    || req.operation == Operation::AgentClose as i32;
                let result = handle_session(
                    req,
                    &mut engine,
                    &pty,
                    &mut control,
                    &mut presence,
                    &mut info,
                    &mut snapshots,
                    &mut scrollback,
                    gate,
                    authorization,
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
#[allow(clippy::too_many_arguments)]
fn handle_session(
    req: Request,
    engine: &mut Engine,
    pty: &Session,
    control: &mut Control,
    presence: &mut DesktopPresence,
    info: &mut SessionInfo,
    snapshots: &mut VecDeque<Snapshot>,
    scrollback: &mut scrollback::Views,
    gate: Option<ExecutionGate>,
    authorization: Option<Arc<ai_terminal_agent_runtime::host::AuthorizationPermit>>,
) -> Result<Reply> {
    let op = Operation::try_from(req.operation)?;
    if req.session_epoch != 0 && req.session_epoch != info.epoch {
        bail!("stale session epoch")
    }
    presence.touch(req.client);
    scrollback.touch(req.client);
    let guarded = matches!(
        op,
        Operation::AgentAcquire
            | Operation::AgentWrite
            | Operation::AgentClose
            | Operation::AgentResize
            | Operation::AgentRelease
    );
    let permit = if guarded {
        Some(
            gate.as_ref()
                .context("internal_agent_permit_required")?
                .lock()
                .unwrap(),
        )
    } else {
        None
    };
    if let Some(permit) = &permit {
        anyhow::ensure!(**permit || op == Operation::AgentRelease, "agent_cancelled");
    }
    if matches!(
        op,
        Operation::AgentAcquire
            | Operation::AgentWrite
            | Operation::AgentClose
            | Operation::AgentResize
    ) {
        anyhow::ensure!(
            req.manual_revision == info.manual_revision,
            "manual_input_preempted_agent"
        );
        if op != Operation::AgentAcquire {
            control.check(req.client, req.control_epoch)?;
            anyhow::ensure!(
                presence.active(),
                "Desktop is detached; terminal is read-only"
            );
        }
    }
    if matches!(
        op,
        Operation::AgentWrite | Operation::AgentClose | Operation::AgentResize
    ) && let Some(permit) = &authorization
    {
        authorization::commit(permit, info, engine.snapshot_revision())?;
    }
    let mut reply = Reply::default();
    match op {
        Operation::Scrollback => {
            reply = scrollback.read(&req, engine)?;
        }
        Operation::ReleaseScrollback => {
            scrollback.release_view(req.client, req.scrollback_id);
        }
        Operation::AttachDesktop => {
            if info.exited {
                bail!("session exited")
            }
            control.acquire(req.client)?;
            presence.attach(req.client)?;
        }
        Operation::Acquire | Operation::AgentAcquire => {
            if info.exited {
                bail!("session exited")
            }
            control.acquire(req.client)?;
        }
        Operation::Detach | Operation::AgentRelease => {
            scrollback.release(req.client);
            control.release(req.client);
            presence.detach(req.client);
        }
        Operation::AgentWrite => {
            anyhow::ensure!(!info.exited, "session exited");
            anyhow::ensure!(
                req.input_kind != 1 || !req.text.contains(['\n', '\r']) || engine.bracketed_paste(),
                "multiline_requires_bracketed_paste"
            );
            let bytes = encode_input(&req, engine)?;
            pty.write(bytes)?;
            authorization::input(
                info,
                authorization.as_ref().and_then(|p| p.submitted_command()),
            );
        }
        Operation::ObserveTerminal => {
            reply.history = vec![serde_json::to_string(&engine.read_view(12000, 512 * 1024))?];
            reply.snapshot = Some(engine.snapshot());
        }
        Operation::AssistantInput => {
            if info.exited {
                bail!("session exited");
            }
            if !presence.active() {
                bail!("Desktop is detached; terminal is read-only")
            }
            control.check(req.client, req.control_epoch)?;
            if req.input_seq != control.metadata(req.client).2 {
                bail!("manual input occurred; stale AI input cancelled");
            }
            let bytes = encode_input(&req, engine)?;
            pty.write(bytes)?;
            authorization::input(info, None);
        }
        Operation::Input => {
            let started = Instant::now();
            if info.exited {
                bail!("session exited")
            }
            if !presence.active() {
                bail!("Desktop is detached; terminal is read-only")
            }
            let signature = req.encode_to_vec();
            if !control.input(req.client, req.control_epoch, req.input_seq, &signature)? {
                let bytes = encode_input(&req, engine)?;
                // Application-requested focus reports are protocol traffic, not manual input.
                let focus_notification =
                    engine.focus_reporting() && matches!(bytes.as_slice(), b"\x1b[I" | b"\x1b[O");
                let has_input = !bytes.is_empty();
                let changed = has_input && !focus_notification;
                pty.write(bytes)?;
                if has_input {
                    authorization::input(info, None);
                }
                if changed {
                    info.manual_revision += 1;
                }
                control.commit(req.client, req.input_seq, signature);
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
        Operation::Resize | Operation::AgentResize => {
            if !presence.active() {
                bail!("Desktop is detached; terminal is read-only")
            }
            control.check(req.client, req.control_epoch)?;
            let rows = u16::try_from(req.rows)?;
            let cols = u16::try_from(req.cols)?;
            ai_terminal_engine::check_size(rows, cols)?;
            pty.resize(rows, cols)?;
            let old_revision = engine.snapshot_revision();
            engine.resize(rows, cols)?;
            if op == Operation::Resize && old_revision != engine.snapshot_revision() {
                info.manual_revision += 1;
            }
            let snapshot = engine.snapshot();
            if let Err(e) = publish_snapshot(snapshot, snapshots) {
                eprintln!("session {}: skipped invalid resize frame: {e}", info.id);
            }
        }
        Operation::History => {
            // Point-in-time read, not a independently reflowed interactive viewport.
            let (lines, truncated) =
                engine.history(req.history_offset as usize, req.history_limit as usize);
            reply.history = lines;
            reply.history_truncated = truncated;
        }
        Operation::Close | Operation::AgentClose => {}
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
    (info.controller, info.control_epoch, info.next_input_seq) = control.metadata(req.client);
    info.desktop_attached = presence.active();
    info.availability_epoch = presence.epoch;
    reply.info = Some(info.clone());
    Ok(reply)
}

fn publish_snapshot(
    snapshot: Snapshot,
    snapshots: &mut VecDeque<Snapshot>,
) -> Result<(), ProtocolError> {
    snapshot.validate()?;
    snapshots.push_back(snapshot);
    if snapshots.len() > 16 {
        snapshots.pop_front();
    }
    Ok(())
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
        3 => {
            let modifiers: Vec<String> = serde_json::from_str(&request.text)?;
            let repeats = request.key_repeat.max(1);
            anyhow::ensure!(repeats <= 20, "key_repeat_limit");
            Ok(
                crate::keys::encode(&request.key, &modifiers, engine.application_cursor())?
                    .repeat(repeats as usize),
            )
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

#[cfg(test)]
mod service_tests {
    use super::*;

    /// A fresh session actor and real PTY, with no listener or existing Desktop service.
    #[cfg(unix)]
    struct FocusSession {
        actor: Actor,
        worker: Option<thread::JoinHandle<()>>,
        base: Request,
    }

    #[cfg(unix)]
    impl FocusSession {
        fn new(workload: &str) -> Self {
            let command = [
                OsString::from("/bin/sh"),
                OsString::from("-c"),
                OsString::from(workload),
            ];
            let pty = Session::spawn(&command, None, 24, 80).unwrap();
            let engine = Engine::new(24, 80, 7).unwrap();
            let (actor, rx) = mpsc::sync_channel(128);
            let worker = thread::spawn(move || {
                session_loop("focus-test".into(), PathBuf::new(), engine, pty, 1, rx)
            });
            let base = Request {
                client: 1,
                session_epoch: 7,
                control_epoch: 1,
                ..Default::default()
            };
            let session = Self {
                actor,
                worker: Some(worker),
                base,
            };
            let attached = session.call(Request {
                operation: Operation::AttachDesktop as i32,
                ..session.base.clone()
            });
            assert!(attached.error.is_empty(), "{}", attached.error);
            session.wait_for("READY");
            session
        }

        fn call(&self, request: Request) -> Reply {
            request_actor(&self.actor, request).unwrap()
        }

        fn agent(&self, request: Request, gate: Option<ExecutionGate>) -> Reply {
            request_actor_guarded(&self.actor, request, gate).unwrap()
        }

        fn wait_for(&self, text: &str) -> Reply {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                let reply = self.call(Request {
                    operation: Operation::Poll as i32,
                    ..self.base.clone()
                });
                assert!(reply.error.is_empty(), "{}", reply.error);
                let frame = reply.snapshot.as_ref().unwrap();
                let screen: String = frame.cells.iter().map(|cell| cell.text.as_str()).collect();
                if screen.contains(text) {
                    return reply;
                }
                assert!(Instant::now() < until, "missing {text:?}: {screen:?}");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[cfg(unix)]
    impl Drop for FocusSession {
        fn drop(&mut self) {
            let _ = request_actor(
                &self.actor,
                Request {
                    operation: Operation::Close as i32,
                    ..self.base.clone()
                },
            );
            let _ = self.worker.take().unwrap().join();
        }
    }

    #[cfg(unix)]
    #[test]
    fn focus_notifications_preserve_agent_fence_and_reach_real_pty() {
        let session = FocusSession::new(
            r"stty raw -echo; printf '\033[?1004hREADY'; dd bs=1 count=7 2>/dev/null | od -An -tx1; printf '\033[?1004lDONE'; exec cat",
        );
        let initial = session.wait_for("READY");
        assert_ne!(initial.snapshot.unwrap().input_modes & 4, 0);
        let initial = initial.info.unwrap();
        let focus = Request {
            operation: Operation::Input as i32,
            input_seq: 1,
            input: b"\x1b[I".to_vec(),
            ..session.base.clone()
        };
        let gained = session.call(focus.clone());
        assert!(gained.error.is_empty(), "{}", gained.error);
        let gate = Arc::new(Mutex::new(true));
        let agent = Request {
            operation: Operation::AgentAcquire as i32,
            manual_revision: initial.manual_revision,
            ..session.base.clone()
        };
        let acquired = session.agent(agent.clone(), Some(gate.clone()));
        assert_eq!(
            gained.info.as_ref().unwrap().manual_revision,
            initial.manual_revision,
            "focus notification invalidated Agent fence: {}",
            acquired.error
        );
        assert!(acquired.error.is_empty(), "{}", acquired.error);
        assert_eq!(gained.accepted_input_seq, 1);
        let info = gained.info.unwrap();
        assert_eq!(info.next_input_seq, 2);
        assert_eq!(info.control_epoch, initial.control_epoch);
        assert_eq!(info.availability_epoch, initial.availability_epoch);
        assert!(info.desktop_attached);
        // An identical retry acknowledges the sequence without writing a second focus event.
        let duplicate = session.call(focus.clone());
        assert_eq!(duplicate.accepted_input_seq, 1);
        assert_eq!(duplicate.info.unwrap().next_input_seq, 2);
        let conflicting = session.call(Request {
            input: b"\x1b[O".to_vec(),
            ..focus
        });
        assert!(conflicting.error.contains("conflicting retry"));
        let write = Request {
            operation: Operation::AgentWrite as i32,
            input: b"x".to_vec(),
            ..agent.clone()
        };
        assert_eq!(
            session.agent(write.clone(), None).error,
            "internal_agent_permit_required"
        );
        assert_eq!(
            session
                .agent(write.clone(), Some(Arc::new(Mutex::new(false))))
                .error,
            "agent_cancelled"
        );
        assert!(session.agent(write, Some(gate.clone())).error.is_empty());
        let lost = session.call(Request {
            operation: Operation::Input as i32,
            input_seq: 2,
            input: b"\x1b[O".to_vec(),
            ..session.base.clone()
        });
        assert!(lost.error.is_empty(), "{}", lost.error);
        assert_eq!(lost.info.unwrap().manual_revision, initial.manual_revision);
        // The workload reports bytes read from its PTY, including the guarded Agent write.
        let done = session.wait_for("DONE");
        let frame = done.snapshot.unwrap();
        let screen: String = frame.cells.iter().map(|cell| cell.text.as_str()).collect();
        let normalized = screen.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(normalized.contains("1b 5b 49 78 1b 5b 4f"), "{screen:?}");
        assert_eq!(frame.input_modes & 4, 0);
        for (index, bytes) in [b"\x1b[I".as_slice(), b"\x1b[O".as_slice()]
            .into_iter()
            .enumerate()
        {
            let reply = session.call(Request {
                operation: Operation::Input as i32,
                input_seq: 3 + index as u64,
                input: bytes.to_vec(),
                ..session.base.clone()
            });
            assert!(reply.error.is_empty(), "{}", reply.error);
            assert_eq!(reply.info.unwrap().manual_revision, index as u64 + 1);
            assert_eq!(
                session.agent(agent.clone(), Some(gate.clone())).error,
                "manual_input_preempted_agent"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn focus_notifications_keep_input_guards_and_manual_input_preemption() {
        let session =
            FocusSession::new(r"stty raw -echo; printf '\033[?1004h\033[?2004hREADY'; exec cat");
        let focus = Request {
            operation: Operation::Input as i32,
            input_seq: 1,
            input: b"\x1b[I".to_vec(),
            ..session.base.clone()
        };
        for (request, expected) in [
            (
                Request {
                    input_seq: 2,
                    ..focus.clone()
                },
                "input sequence gap",
            ),
            (
                Request {
                    control_epoch: 2,
                    ..focus.clone()
                },
                "input stream expired",
            ),
            (
                Request {
                    session_epoch: 8,
                    ..focus.clone()
                },
                "stale session epoch",
            ),
            (
                Request {
                    client: 2,
                    ..focus.clone()
                },
                "input stream expired",
            ),
        ] {
            let rejected = session.call(request);
            assert!(rejected.error.contains(expected), "{}", rejected.error);
        }
        let accepted = session.call(focus.clone());
        assert!(accepted.error.is_empty(), "{}", accepted.error);
        assert_eq!(accepted.info.unwrap().manual_revision, 0);

        // No broad escape exemption: ordinary bytes, encoded paste, mouse, partial,
        // malformed, mixed, and concatenated focus notifications remain manual input.
        let mut inputs: Vec<Request> = [
            b"x".as_slice(),
            b"\x1b[200~pasted\x1b[201~",
            b"\x1b[<0;1;1M",
            b"\x1b[",
            b"\x1b[1I",
            b"\x1b[Ix",
            b"\x1b[I\x1b[O",
            b"\x1b[O\r",
        ]
        .into_iter()
        .map(|bytes| Request {
            input: bytes.to_vec(),
            ..focus.clone()
        })
        .collect();
        inputs.push(Request {
            input_kind: 1,
            text: "paste".into(),
            input: Vec::new(),
            ..focus.clone()
        });
        let gate = Arc::new(Mutex::new(true));
        let agent = Request {
            operation: Operation::AgentWrite as i32,
            input: b"agent".to_vec(),
            ..session.base.clone()
        };
        let count = inputs.len() as u64;
        for (index, mut request) in inputs.into_iter().enumerate() {
            request.input_seq = index as u64 + 2;
            let accepted = session.call(request);
            assert!(accepted.error.is_empty(), "{}", accepted.error);
            assert_eq!(accepted.info.unwrap().manual_revision, index as u64 + 1);
            assert_eq!(
                session.agent(agent.clone(), Some(gate.clone())).error,
                "manual_input_preempted_agent"
            );
        }
        let resize = Request {
            operation: Operation::Resize as i32,
            rows: 24,
            cols: 80,
            ..session.base.clone()
        };
        assert_eq!(
            session.call(resize.clone()).info.unwrap().manual_revision,
            count
        );
        let resized = session.call(Request { cols: 81, ..resize });
        assert!(resized.error.is_empty(), "{}", resized.error);
        assert_eq!(resized.info.unwrap().manual_revision, count + 1);
        assert_eq!(
            session.agent(agent, Some(gate)).error,
            "manual_input_preempted_agent"
        );
        session.call(Request {
            operation: Operation::Detach as i32,
            ..session.base.clone()
        });
        let detached = session.call(Request {
            input_seq: count + 2,
            ..focus
        });
        assert_eq!(detached.error, "Desktop is detached; terminal is read-only");
    }

    fn test_host(dir: &Path, async_runtime: &tokio::runtime::Runtime) -> Arc<Host> {
        let state = dir.join("state");
        crate::service::secure_dir(&state).unwrap();
        let agents = ai_terminal_agent_runtime::host::AgentHost::new(
            Arc::new(
                ai_terminal_agent_runtime::store::Store::open(&state.join("data/agent.sqlite3"))
                    .unwrap(),
            ),
            async_runtime.handle().clone(),
        );
        Arc::new(Host {
            agents,
            state_dir: state.clone(),
            recent_directories: Mutex::new(crate::recent_directories::RecentDirectories::new(
                &state,
            )),
            account: crate::account::AccountManager::new(&state).unwrap(),
            config: crate::config::ConfigService::open(&state).unwrap(),
            assistant: crate::assistant::Assistant::default(),
            sessions: Mutex::new(HashMap::new()),
            session_order: Mutex::new(Vec::new()),
            owners: Mutex::new(HashMap::new()),
            stop: Arc::new(AtomicBool::new(false)),
            workers: AtomicUsize::new(0),
        })
    }

    #[cfg(unix)]
    #[test]
    fn recent_directory_create_validates_paths_and_observes_real_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let host = test_host(dir.path(), &runtime);
        let initial = dir.path().join("literal ; $(touch should-not-exist)");
        let next = dir.path().join("next");
        fs::create_dir(&initial).unwrap();
        fs::create_dir(&next).unwrap();
        let create = |cwd: String| {
            dispatch(
                &host,
                Request {
                    operation: Operation::Create as i32,
                    client: 1,
                    cwd,
                    command: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        "read -r ignored; cd -- \"$1\"; read -r ignored".into(),
                        "cwd-test".into(),
                        next.to_str().unwrap().into(),
                    ],
                    rows: 24,
                    cols: 80,
                    ..Default::default()
                },
            )
        };
        let info = create(initial.to_str().unwrap().into())
            .unwrap()
            .info
            .unwrap();
        let initial = initial.canonicalize().unwrap();
        assert_eq!(info.cwd, initial.to_str().unwrap());
        assert_eq!(
            crate::process::cwd(&info).unwrap().canonicalize().unwrap(),
            initial
        );
        let mut wrong = info.clone();
        wrong.process_identity = "wrong-process".into();
        assert!(crate::process::cwd(&wrong).is_none());
        wrong = info.clone();
        wrong.exited = true;
        assert!(crate::process::cwd(&wrong).is_none());
        let recorder = spawn_directory_recorder(Arc::downgrade(&host));
        // Release the shell's read: the next directory was passed as argv, never interpolated.
        let actor = host.sessions.lock().unwrap().get(&info.id).unwrap().clone();
        let attached = request_actor(
            &actor,
            Request {
                operation: Operation::AttachDesktop as i32,
                client: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(attached.error.is_empty(), "{}", attached.error);
        let input = request_actor(
            &actor,
            Request {
                operation: Operation::Input as i32,
                client: 1,
                control_epoch: info.control_epoch,
                input_seq: info.next_input_seq,
                input: b"\n".to_vec(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(input.error.is_empty(), "{}", input.error);
        let next = next.canonicalize().unwrap().to_str().unwrap().to_string();
        let until = Instant::now() + Duration::from_secs(8);
        loop {
            let list = dispatch(
                &host,
                Request {
                    client: 1,
                    ..Default::default()
                },
            )
            .unwrap();
            if list.recent_directories.first() == Some(&next) {
                break;
            }
            assert!(Instant::now() < until, "OS cwd change was not recorded");
            thread::sleep(Duration::from_millis(100));
        }
        let list = host.recent_directories.lock().unwrap().list("").unwrap();
        assert_eq!(list, [next.clone(), initial.to_str().unwrap().to_string()]);
        let file = dir.path().join("file");
        fs::write(&file, "x").unwrap();
        assert!(create(file.to_str().unwrap().into()).is_err());
        assert!(create(dir.path().join("missing").to_str().unwrap().into()).is_err());
        assert_eq!(host.sessions.lock().unwrap().len(), 1);
        assert_eq!(
            host.recent_directories.lock().unwrap().list("").unwrap(),
            list
        );
        // Another creation uses the initial directory again. Polling the idle first shell
        // must not promote its unchanged cwd above that newer use.
        let second = create(initial.to_str().unwrap().into())
            .unwrap()
            .info
            .unwrap();
        thread::sleep(Duration::from_millis(2200));
        assert_eq!(
            host.recent_directories.lock().unwrap().list("").unwrap(),
            [initial.to_str().unwrap().to_string(), next.clone()]
        );
        host.stop.store(true, Ordering::Release);
        recorder.join().unwrap();
        dispatch(
            &host,
            Request {
                operation: Operation::Close as i32,
                client: 1,
                session: second.id,
                ..Default::default()
            },
        )
        .unwrap();
        dispatch(
            &host,
            Request {
                operation: Operation::Close as i32,
                client: 1,
                session: info.id,
                ..Default::default()
            },
        )
        .unwrap();
        fs::remove_dir(&next).unwrap();
        assert!(create(next).is_err());
        // The actual working tree and history survive rejection; no second session was started.
        assert!(host.sessions.lock().unwrap().is_empty());
        let default = create(String::new()).unwrap().info.unwrap();
        assert_eq!(
            default.cwd,
            std::env::current_dir()
                .unwrap()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
        dispatch(
            &host,
            Request {
                operation: Operation::Close as i32,
                client: 1,
                session: default.id,
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn session_list_returns_newest_first_and_removes_closed_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let async_runtime = tokio::runtime::Runtime::new().unwrap();
        let host = test_host(dir.path(), &async_runtime);
        // Deliberately different from ID order; the latest session has exited.
        let mut workers = Vec::new();
        for (id, exited) in [("z-old", false), ("a-new", false), ("m-exited", true)] {
            let (tx, rx) = mpsc::sync_channel::<ActorMessage>(8);
            host.sessions.lock().unwrap().insert(id.into(), tx);
            host.session_order.lock().unwrap().push(id.into());
            workers.push(thread::spawn(move || {
                while let Ok(ActorMessage { reply, .. }) = rx.recv() {
                    let _ = reply.send(Reply {
                        info: Some(SessionInfo {
                            id: id.into(),
                            exited,
                            ..SessionInfo::default()
                        }),
                        ..Reply::default()
                    });
                }
            }));
        }
        let list = || {
            dispatch(
                &host,
                Request {
                    operation: Operation::List as i32,
                    client: 1,
                    ..Request::default()
                },
            )
            .unwrap()
            .sessions
        };
        let sessions = list();
        assert_eq!(
            sessions.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["m-exited", "a-new", "z-old"]
        );
        assert_eq!(sessions.iter().find(|s| !s.exited).unwrap().id, "a-new");
        dispatch(
            &host,
            Request {
                operation: Operation::Close as i32,
                client: 1,
                session: "a-new".into(),
                ..Request::default()
            },
        )
        .unwrap();
        assert_eq!(list().iter().find(|s| !s.exited).unwrap().id, "z-old");
        assert!(
            !host
                .session_order
                .lock()
                .unwrap()
                .iter()
                .any(|id| id == "a-new")
        );
        drop(host);
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn invalid_intermediate_frame_does_not_poison_the_baseline() {
        let mut engine = Engine::new(2, 8, 1).unwrap();
        let first = engine.snapshot();
        let mut snapshots = VecDeque::from([first.clone()]);
        let mut invalid = first.clone();
        invalid.revision += 1;
        invalid.cells[0].text = "\t".into();
        invalid.seal();
        assert_eq!(
            publish_snapshot(invalid, &mut snapshots),
            Err(ProtocolError::Invalid)
        );
        assert_eq!(snapshots.back(), Some(&first));

        engine.feed(b"ok");
        let recovered = engine.snapshot();
        publish_snapshot(recovered.clone(), &mut snapshots).unwrap();
        assert_eq!(snapshots.back(), Some(&recovered));
    }

    #[test]
    fn closed_actor_and_busy_actor_have_distinct_errors() {
        let (actor, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        assert_eq!(
            request_actor(&actor, Request::default())
                .unwrap_err()
                .to_string(),
            SESSION_CLOSED_ERROR
        );

        let (actor, _receiver) = mpsc::sync_channel(1);
        let (reply, _) = mpsc::sync_channel(1);
        actor
            .try_send(ActorMessage {
                request: Request::default(),
                reply,
                gate: None,
                authorization: None,
            })
            .unwrap();
        assert_eq!(
            request_actor(&actor, Request::default())
                .unwrap_err()
                .to_string(),
            "session busy"
        );
    }

    #[test]
    fn desktop_availability_changes_only_at_first_attach_and_last_detach() {
        let mut presence = DesktopPresence::default();
        assert!(!presence.active());
        presence.attach(1).unwrap();
        let attached_epoch = presence.epoch;
        presence.attach(2).unwrap();
        assert_eq!(presence.epoch, attached_epoch);
        presence.detach(1);
        assert!(presence.active());
        assert_eq!(presence.epoch, attached_epoch);
        presence.clients.insert(2, Instant::now() - DESKTOP_LEASE);
        assert!(presence.expire());
        assert!(!presence.active());
        assert!(presence.epoch > attached_epoch);
    }
}
