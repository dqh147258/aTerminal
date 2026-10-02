//! Durable event/record/action ledger. One connection serializes all mutations.
mod authorization;
#[path = "store/tools.rs"]
mod tools;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub owner: String,
    pub desktop: String,
    pub agent: String,
    pub session: Option<String>,
}
impl Scope {
    fn key(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryItem {
    pub sequence: i64,
    pub id: String,
    pub kind: String,
    pub root_user_message_id: Option<String>,
    pub created_at: i64,
    pub value: Value,
}
#[derive(Serialize, Deserialize)]
pub struct HistoryPage {
    pub snapshot_watermark: i64,
    pub items: Vec<HistoryItem>,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub generation: i64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Cursor {
    scope: String,
    kind: String,
    generation: i64,
    before: i64,
    high: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub metadata: Value,
    pub summary: Option<Value>,
    pub bytes: usize,
    pub pending: bool,
}
#[derive(Serialize)]
pub struct RecordPage {
    pub record: Record,
    pub payload: String,
    pub encoding: String,
    pub next_offset: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserAccepted {
    pub root_user_message_id: String,
    pub run_id: String,
    pub duplicate: bool,
    pub user_message_id: String,
    pub status_message_id: String,
    pub event_sequence: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub state: String,
    pub duplicate: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case")]
pub enum Retention {
    Before { utc_ms: i64 },
    OlderThan { days: u32 },
    KeepLast { count: u64 },
}
#[derive(Serialize)]
pub struct CleanResult {
    pub scope: Scope,
    pub cutoff_utc_ms: Option<i64>,
    pub logical_reclaimed_bytes: u64,
    pub reusable_bytes: u64,
    pub candidates: u64,
    pub deleted: u64,
    pub pinned: u64,
    pub generation: i64,
}
pub struct Store {
    db: Mutex<Connection>,
    cursor_key: [u8; 32],
}
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn id() -> String {
    Uuid::now_v7().to_string()
}
/// Leave room for JSON escaping and report metadata within the event size limit.
pub(crate) fn task_error(error: Option<&str>) -> (Option<&str>, bool) {
    (
        error.map(|text| &text[..text.floor_char_boundary(text.len().min(2048))]),
        error.is_some_and(|text| text.len() > 2048),
    )
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let parent = path.parent().context("database_directory_required")?;
        if !parent.exists() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(parent)?;
        }
        let directory = std::fs::symlink_metadata(parent)?;
        ensure!(
            directory.is_dir() && !directory.file_type().is_symlink(),
            "invalid_database_directory"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                directory.uid() == rustix::process::getuid().as_raw()
                    && directory.mode() & 0o077 == 0,
                "database_directory_must_be_private"
            );
        }
        if !path.exists() {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)?;
        }
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "invalid_database_path"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == rustix::process::getuid().as_raw()
                    && metadata.mode() & 0o077 == 0,
                "database_file_must_be_private"
            );
        }
        let db = Connection::open(path)?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!([0, 1, 2].contains(&version), "unsupported_database_version");
        if version == 1 {
            let backup = path.with_extension("v1-backup.sqlite3");
            if !backup.exists() {
                let mut options = std::fs::OpenOptions::new();
                options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options.open(&backup)?;
                db.backup("main", &backup, None)?;
            }
        }
        if version == 0 {
            let occupied:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')",[],|r|r.get(0))?;
            ensure!(!occupied, "unversioned_database_not_empty");
        }
        let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        db.pragma_update(
            None,
            "max_page_count",
            (2_i64 * 1024 * 1024 * 1024) / page_size,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA auto_vacuum=INCREMENTAL; BEGIN IMMEDIATE;
          CREATE TABLE IF NOT EXISTS image_uploads(id TEXT PRIMARY KEY,scope TEXT NOT NULL,mime TEXT NOT NULL,size INTEGER NOT NULL,body BLOB NOT NULL,created INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS model_usage(id TEXT PRIMARY KEY,scope TEXT NOT NULL,run TEXT NOT NULL,root TEXT NOT NULL,stage TEXT NOT NULL,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value BLOB NOT NULL);
          CREATE TABLE IF NOT EXISTS owner_retention(owner TEXT NOT NULL,desktop TEXT NOT NULL,rule TEXT,PRIMARY KEY(owner,desktop));
          CREATE TABLE IF NOT EXISTS clean_pass(scope TEXT PRIMARY KEY,rule TEXT NOT NULL,high INTEGER NOT NULL,bound INTEGER NOT NULL,after INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS scopes(scope TEXT PRIMARY KEY,generation INTEGER NOT NULL DEFAULT 0,retention TEXT);
          CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,scope TEXT NOT NULL,id TEXT NOT NULL UNIQUE,kind TEXT NOT NULL,root TEXT,at INTEGER NOT NULL,value TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS events_scope_seq ON events(scope,seq DESC);
          CREATE INDEX IF NOT EXISTS events_agent_task ON events(json_extract(value,'$.task_id'),seq DESC) WHERE kind='agent_report';
          CREATE INDEX IF NOT EXISTS events_assistant_run ON events(json_extract(value,'$.run_id'),scope,seq DESC) WHERE kind='assistant';
          CREATE TABLE IF NOT EXISTS users(scope TEXT NOT NULL,request TEXT NOT NULL,hash TEXT NOT NULL,root TEXT NOT NULL,run TEXT NOT NULL,PRIMARY KEY(scope,request));
          CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY,scope TEXT NOT NULL,root TEXT NOT NULL,state TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS blobs(hash TEXT PRIMARY KEY,body BLOB NOT NULL);
          CREATE TABLE IF NOT EXISTS records(id TEXT PRIMARY KEY,scope TEXT NOT NULL,request TEXT NOT NULL,kind TEXT NOT NULL,hash TEXT NOT NULL REFERENCES blobs(hash),metadata TEXT NOT NULL,summary TEXT,pending INTEGER NOT NULL,event INTEGER REFERENCES events(seq) ON DELETE CASCADE,UNIQUE(scope,request));
          CREATE INDEX IF NOT EXISTS records_event ON records(event);
          CREATE TABLE IF NOT EXISTS actions(scope TEXT NOT NULL,id TEXT NOT NULL,run TEXT NOT NULL,hash TEXT NOT NULL,state TEXT NOT NULL,PRIMARY KEY(scope,id));
          CREATE TABLE IF NOT EXISTS pins(run TEXT NOT NULL,event INTEGER NOT NULL REFERENCES events(seq) ON DELETE CASCADE,PRIMARY KEY(run,event));
          CREATE TABLE IF NOT EXISTS projections(scope TEXT PRIMARY KEY,generation INTEGER NOT NULL,covered INTEGER NOT NULL,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS agents(binding TEXT PRIMARY KEY,scope TEXT NOT NULL UNIQUE);
          CREATE TABLE IF NOT EXISTS user_inputs(scope TEXT NOT NULL,request TEXT NOT NULL,user_id TEXT NOT NULL,status_id TEXT NOT NULL,event_seq INTEGER NOT NULL,PRIMARY KEY(scope,request));
          CREATE TABLE IF NOT EXISTS analyses(id TEXT PRIMARY KEY,scope TEXT NOT NULL,record TEXT NOT NULL,at INTEGER NOT NULL,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS unit_updates(seq INTEGER PRIMARY KEY AUTOINCREMENT,unit TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,value TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS updates_unit ON unit_updates(unit,seq);
          CREATE TABLE IF NOT EXISTS retained(scope TEXT PRIMARY KEY,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS delegations(scope TEXT NOT NULL,request TEXT NOT NULL,hash TEXT NOT NULL,root TEXT NOT NULL,run TEXT NOT NULL,PRIMARY KEY(scope,request));
          CREATE INDEX IF NOT EXISTS delegations_run ON delegations(run);
          UPDATE events SET kind='archive_index' WHERE seq IN (SELECT event FROM records WHERE kind IN ('context_index','history_index'));
          UPDATE actions SET state='unknown' WHERE state IN ('prepared','accepted');
          UPDATE runs SET state='orphaned' WHERE state IN ('running','paused');
          DELETE FROM pins; PRAGMA user_version=2; COMMIT;")?;
        authorization::initialize(&db)?;
        tools::interrupt_native_commands(&db)?;
        let key: Option<Vec<u8>> = db
            .query_row("SELECT value FROM meta WHERE key='cursor_key'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let key = match key {
            Some(key) => key,
            None => {
                let key = blake3::hash(format!("{}{}", id(), id()).as_bytes())
                    .as_bytes()
                    .to_vec();
                db.execute("INSERT INTO meta VALUES('cursor_key',?1)", [&key])?;
                key
            }
        };
        let cursor_key = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid_cursor_key"))?;
        Ok(Self {
            db: Mutex::new(db),
            cursor_key,
        })
    }
    pub fn append_status(&self, scope: &Scope, status: Value) -> Result<()> {
        let key = scope.key()?;
        let db = self.db.lock().unwrap();
        ensure_scope(&db, &key)?;
        insert_status_if_changed(&db, &key, "pty_status", None, &status)?;
        Ok(())
    }
    /// Only the authenticated user-message path may call this entry point.
    pub fn accept_user(
        &self,
        scope: &Scope,
        request: &str,
        message: &str,
        status: Value,
    ) -> Result<UserAccepted> {
        self.accept_user_in_run(scope, request, message, status, None)
    }
    pub fn accept_user_in_run(
        &self,
        scope: &Scope,
        request: &str,
        message: &str,
        status: Value,
        active: Option<&str>,
    ) -> Result<UserAccepted> {
        self.accept_user_authorized(scope, request, message, status, active, false)
    }
    pub fn accept_user_authorized(
        &self,
        scope: &Scope,
        request: &str,
        message: &str,
        status: Value,
        active: Option<&str>,
        allow_write: bool,
    ) -> Result<UserAccepted> {
        self.accept_user_checked(scope, request, message, status, active, allow_write, || {
            Ok(())
        })
    }
    /// Validate admission after duplicate detection but before persisting a new message.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn accept_user_checked(
        &self,
        scope: &Scope,
        request: &str,
        message: &str,
        status: Value,
        active: Option<&str>,
        allow_write: bool,
        admit: impl FnOnce() -> Result<()>,
    ) -> Result<UserAccepted> {
        self.accept_user_images(
            scope,
            request,
            message,
            status,
            active,
            allow_write,
            &[],
            admit,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn accept_user_images(
        &self,
        scope: &Scope,
        request: &str,
        message: &str,
        status: Value,
        active: Option<&str>,
        allow_write: bool,
        images: &[String],
        admit: impl FnOnce() -> Result<()>,
    ) -> Result<UserAccepted> {
        ensure!(
            !request.is_empty()
                && request.len() <= 128
                && (!message.trim().is_empty() || !images.is_empty())
                && message.len() <= 16000,
            "invalid_user_message"
        );
        let key = scope.key()?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        ensure_scope(&tx, &key)?;
        ensure!(images.len() <= 4, "image_count_limit");
        let mut pictures = Vec::new();
        let mut total = 0usize;
        for upload in images {
            let (mime, size, bytes): (String, i64, Vec<u8>) = tx
                .query_row(
                    "SELECT mime,size,body FROM image_uploads WHERE id=?1 AND scope=?2",
                    params![upload, key],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .context("image_upload_expired_retry")?;
            ensure!(bytes.len() as i64 == size, "image_upload_incomplete");
            validate_image(&mime, &bytes)?;
            total += size as usize;
            ensure!(total <= 8 * 1024 * 1024, "image_total_limit");
            pictures.push((mime, bytes));
        }
        let hashes: Vec<_> = pictures
            .iter()
            .map(|(mime, bytes)| (mime, blake3::hash(bytes).to_hex().to_string()))
            .collect();
        // Preserve old text-only idempotency hashes; re-uploaded identical bytes retain identity.
        let payload = if hashes.is_empty() {
            serde_json::to_string(&(message, allow_write))?
        } else {
            serde_json::to_string(&(message, allow_write, hashes))?
        };
        let hash = blake3::hash(payload.as_bytes()).to_hex().to_string();
        if let Some((old, root, run)) = tx
            .query_row(
                "SELECT hash,root,run FROM users WHERE scope=?1 AND request=?2",
                params![key, request],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        {
            ensure!(old == hash, "request_payload_conflict");
            let ids=tx.query_row("SELECT user_id,status_id,event_seq FROM user_inputs WHERE scope=?1 AND request=?2",params![key,request],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?))).optional()?.unwrap_or_else(||(root.clone(),String::new(),0));
            return Ok(UserAccepted {
                root_user_message_id: root,
                run_id: run,
                duplicate: true,
                user_message_id: ids.0,
                status_message_id: ids.1,
                event_sequence: ids.2,
            });
        }
        admit()?;
        let user_id = id();
        let (run, root) = if let Some(run) = active {
            let root = tx
                .query_row(
                    "SELECT root FROM runs WHERE id=?1 AND scope=?2 AND state='running'",
                    params![run, key],
                    |r| r.get::<_, String>(0),
                )
                .context("run_not_active")?;
            (run.to_owned(), root)
        } else {
            (id(), user_id.clone())
        };
        let status_seq =
            insert_status_if_changed(&tx, &key, "pty_status_snapshot", Some(&root), &status)?;
        let status_id = tx.query_row("SELECT id FROM events WHERE seq=?1", [status_seq], |r| {
            r.get::<_, String>(0)
        })?;
        tx.execute(
            "INSERT INTO events(scope,id,kind,root,at,value) VALUES(?1,?2,'user',?3,?4,?5)",
            params![
                key,
                user_id,
                root,
                now(),
                json!({"message":message}).to_string()
            ],
        )?;
        let seq = tx.last_insert_rowid();
        let mut refs = Vec::new();
        for (index, (mime, bytes)) in pictures.iter().enumerate() {
            let record_id = id();
            let hash = blake3::hash(bytes).to_hex().to_string();
            let meta = json!({"binary":true,"mime_type":mime,"source":"user_image","history_unit_id":user_id});
            tx.execute(
                "INSERT OR IGNORE INTO blobs VALUES(?1,?2)",
                params![hash, bytes],
            )?;
            tx.execute(
                "INSERT INTO records VALUES(?1,?2,?3,'image',?4,?5,NULL,0,?6)",
                params![
                    record_id,
                    key,
                    format!("user/{request}/image/{index}"),
                    hash,
                    meta.to_string(),
                    seq
                ],
            )?;
            refs.push(json!({"record_id":record_id,"media_type":mime,"bytes":bytes.len()}));
        }
        if !refs.is_empty() {
            tx.execute(
                "UPDATE events SET value=?2 WHERE seq=?1",
                params![seq, json!({"message":message,"images":refs}).to_string()],
            )?;
        }
        tx.execute(
            "INSERT INTO users VALUES(?1,?2,?3,?4,?5)",
            params![key, request, hash, root, run],
        )?;
        tx.execute(
            "INSERT INTO user_inputs VALUES(?1,?2,?3,?4,?5)",
            params![key, request, user_id, status_id, seq],
        )?;
        if active.is_none() {
            tx.execute(
                "INSERT INTO runs VALUES(?1,?2,?3,'running')",
                params![run, key, root],
            )?;
        }
        tx.execute("INSERT INTO pins VALUES(?1,?2)", params![run, seq])?;
        tx.execute(
            "INSERT OR IGNORE INTO pins VALUES(?1,?2)",
            params![run, status_seq],
        )?;
        tx.commit()?;
        Ok(UserAccepted {
            root_user_message_id: root,
            run_id: run,
            duplicate: false,
            user_message_id: user_id,
            status_message_id: status_id,
            event_sequence: seq,
        })
    }
    pub fn delegate(
        &self,
        scope: &Scope,
        parent_root: &str,
        request: &str,
        message: &str,
        status: Value,
        active: Option<&str>,
    ) -> Result<UserAccepted> {
        ensure!(
            !message.trim().is_empty() && message.len() <= 16000,
            "invalid_agent_message"
        );
        let key = scope.key()?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        ensure_scope(&tx, &key)?;
        let authorized:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE root=?1 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3)",params![parent_root,scope.owner,scope.desktop],|r|r.get(0))?;
        ensure!(authorized, "root_user_message_required");
        let hash = blake3::hash(message.as_bytes()).to_hex().to_string();
        if let Some((old, run)) = tx
            .query_row(
                "SELECT hash,run FROM delegations WHERE scope=?1 AND request=?2",
                params![key, request],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure!(hash == old, "delegation_payload_conflict");
            return Ok(UserAccepted {
                root_user_message_id: parent_root.into(),
                run_id: run,
                duplicate: true,
                user_message_id: String::new(),
                status_message_id: String::new(),
                event_sequence: 0,
            });
        }
        let run = active.map(str::to_owned).unwrap_or_else(id);
        let seq =
            insert_status_if_changed(&tx, &key, "pty_status_snapshot", Some(parent_root), &status)?;
        let status_id = tx.query_row("SELECT id FROM events WHERE seq=?1", [seq], |r| {
            r.get::<_, String>(0)
        })?;
        let task_id = id();
        tx.execute(
            "INSERT INTO events(scope,id,kind,root,at,value) VALUES(?1,?2,'agent_report',?3,?4,?5)",
            params![
                key,
                task_id,
                parent_root,
                now(),
                json!({"message":message,"source":"delegated_task"}).to_string()
            ],
        )?;
        let end = tx.last_insert_rowid();
        if active.is_none() {
            tx.execute(
                "INSERT INTO runs VALUES(?1,?2,?3,'running')",
                params![run, key, parent_root],
            )?;
        }
        tx.execute(
            "INSERT INTO delegations VALUES(?1,?2,?3,?4,?5)",
            params![key, request, hash, parent_root, run],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO pins VALUES(?1,?2)",
            params![run, seq],
        )?;
        tx.execute("INSERT INTO pins VALUES(?1,?2)", params![run, end])?;
        tx.commit()?;
        Ok(UserAccepted {
            root_user_message_id: parent_root.into(),
            run_id: run,
            duplicate: false,
            user_message_id: task_id,
            status_message_id: status_id,
            event_sequence: end,
        })
    }
    pub fn agent(&self, owner: &str, desktop: &str, session: Option<&str>) -> Result<Scope> {
        let binding = serde_json::to_string(&(owner, desktop, session))?;
        let db = self.db.lock().unwrap();
        if let Some(scope) = db
            .query_row(
                "SELECT scope FROM agents WHERE binding=?1",
                [&binding],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(serde_json::from_str(&scope)?);
        }
        let scope = Scope {
            owner: owner.into(),
            desktop: desktop.into(),
            agent: id(),
            session: session.map(str::to_owned),
        };
        db.execute(
            "INSERT INTO agents VALUES(?1,?2)",
            params![binding, scope.key()?],
        )?;
        ensure_scope(&db, &scope.key()?)?;
        Ok(scope)
    }
    pub fn find_agent(
        &self,
        owner: &str,
        desktop: &str,
        session: Option<&str>,
    ) -> Result<Option<Scope>> {
        let db = self.db.lock().unwrap();
        let binding = serde_json::to_string(&(owner, desktop, session))?;
        let raw: Option<String> = db
            .query_row(
                "SELECT scope FROM agents WHERE binding=?1",
                [binding],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|raw| Ok(serde_json::from_str(&raw)?)).transpose()
    }
    /// A retry uses the same binding; the legacy (owner, desktop, None) binding is untouched.
    pub fn create_global(&self, owner: &str, desktop: &str, request: &str) -> Result<Scope> {
        ensure!(
            !request.is_empty() && request.len() <= 128,
            "invalid_request_id"
        );
        let binding = serde_json::to_string(&(owner, desktop, "global", request))?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let raw: Option<String> = tx
            .query_row(
                "SELECT scope FROM agents WHERE binding=?1",
                [&binding],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            return Ok(serde_json::from_str(&raw)?);
        }
        let scope = Scope {
            owner: owner.into(),
            desktop: desktop.into(),
            agent: id(),
            session: None,
        };
        tx.execute(
            "INSERT INTO agents VALUES(?1,?2)",
            params![binding, scope.key()?],
        )?;
        ensure_scope(&tx, &scope.key()?)?;
        tx.commit()?;
        Ok(scope)
    }
    /// Stable rowid pagination includes old empty default conversations without duplicating history.
    pub fn global_page(&self, owner: &str, desktop: &str, before: Option<i64>) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let mut query = db.prepare("SELECT rowid,scope,binding FROM agents WHERE json_extract(scope,'$.owner')=?1 AND json_extract(scope,'$.desktop')=?2 AND json_extract(scope,'$.session') IS NULL AND rowid<?3 ORDER BY rowid DESC LIMIT 25")?;
        let rows = query
            .query_map(params![owner, desktop, before.unwrap_or(i64::MAX)], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = rows.len() > 24;
        let mut result = Vec::new();
        let mut next = None;
        let legacy = serde_json::to_string(&(owner, desktop, Option::<String>::None))?;
        for (row, key, binding) in rows.into_iter().take(24) {
            let scope: Scope = serde_json::from_str(&key)?;
            let title: Option<String> = db.query_row("SELECT substr(coalesce(json_extract(value,'$.message'),json_extract(value,'$.text'),''),1,48) FROM events WHERE scope=?1 AND kind='user' ORDER BY seq LIMIT 1", [&key], |r| r.get(0)).optional()?;
            let recent: Option<(i64, String)> = db.query_row("SELECT at,substr(coalesce(json_extract(value,'$.text'),json_extract(value,'$.message'),''),1,160) FROM events WHERE scope=?1 AND kind IN ('user','assistant','interaction') ORDER BY seq DESC LIMIT 1", [&key], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let reply: i64 = db.query_row("SELECT coalesce(max(seq),0) FROM events WHERE scope=?1 AND kind IN ('assistant','interaction') AND length(coalesce(json_extract(value,'$.text'),''))>0", [&key], |r| r.get(0))?;
            let (updated, preview) = recent.unwrap_or_default();
            result.push(json!({"scope":scope,"legacy":binding==legacy,"title":title.filter(|s| !s.is_empty()).unwrap_or_else(|| "新会话".into()),"preview":preview,"updated_at":updated,"last_reply_sequence":reply}));
            next = Some(row);
        }
        Ok(json!({"conversations":result,"cursor":if more { next } else { None }}))
    }
    pub fn agent_by_id(&self, owner: &str, desktop: &str, id: &str) -> Result<Option<Scope>> {
        let db = self.db.lock().unwrap();
        let raw:Option<String>=db.query_row("SELECT scope FROM agents WHERE json_extract(scope,'$.agent')=?1 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3",params![id,owner,desktop],|r|r.get(0)).optional()?;
        raw.map(|raw| Ok(serde_json::from_str(&raw)?)).transpose()
    }
    pub fn agents(&self, owner: &str, desktop: &str) -> Result<Vec<Scope>> {
        let db = self.db.lock().unwrap();
        let mut query=db.prepare("SELECT scope FROM agents WHERE json_extract(scope,'$.owner')=?1 AND json_extract(scope,'$.desktop')=?2 ORDER BY rowid DESC LIMIT 256")?;
        let rows = query
            .query_map(params![owner, desktop], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    /// Maintenance visits all scopes in bounded database pages; UI catalog limits
    /// must never silently exclude older Session histories from an account policy.
    pub fn visit_agents(
        &self,
        owner: &str,
        desktop: &str,
        mut visit: impl FnMut(Scope) -> Result<()>,
    ) -> Result<()> {
        let mut after = 0_i64;
        loop {
            let rows = {
                let db = self.db.lock().unwrap();
                let mut query=db.prepare("SELECT rowid,scope FROM agents WHERE json_extract(scope,'$.owner')=?1 AND json_extract(scope,'$.desktop')=?2 AND rowid>?3 ORDER BY rowid LIMIT 128")?;
                query
                    .query_map(params![owner, desktop, after], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            if rows.is_empty() {
                break;
            }
            for (row, scope) in rows {
                after = row;
                visit(serde_json::from_str(&scope)?)?;
            }
        }
        Ok(())
    }
    fn generation_unlocked(&self, db: &Connection, key: &str) -> Result<i64> {
        Ok(
            db.query_row("SELECT generation FROM scopes WHERE scope=?1", [key], |r| {
                r.get(0)
            })?,
        )
    }
    pub fn latest_run(&self, scope: &Scope) -> Result<Option<Value>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT id,root,state FROM runs WHERE scope=?1 ORDER BY rowid DESC LIMIT 1",[scope.key()?],|r|Ok(json!({"run_id":r.get::<_,String>(0)?,"root_user_message_id":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?}))).optional()?)
    }
    /// A task is a delegated Run, never whichever Run is now latest for its Session.
    pub fn agent_task(
        &self,
        caller: &Scope,
        task_id: &str,
        limit: usize,
    ) -> Result<(Scope, Value)> {
        self.read_agent_task(caller, task_id, limit, None)
    }
    pub fn agent_task_for_run(
        &self,
        caller: &Scope,
        run: &str,
        task_id: &str,
        limit: usize,
    ) -> Result<(Scope, Value)> {
        self.read_agent_task(caller, task_id, limit, Some(run))
    }
    fn read_agent_task(
        &self,
        caller: &Scope,
        task_id: &str,
        limit: usize,
        pin_run: Option<&str>,
    ) -> Result<(Scope, Value)> {
        ensure!(caller.session.is_none(), "global_agent_required");
        ensure!((4..=12288).contains(&limit), "invalid_task_result_limit");
        let db = self.db.lock().unwrap();
        let (key, root, state): (String, String, String) = db
            .query_row(
                "SELECT scope,root,state FROM runs WHERE id=?1
                 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3
                 AND json_extract(scope,'$.session') IS NOT NULL
                 AND EXISTS(SELECT 1 FROM delegations WHERE run=runs.id AND scope=runs.scope)",
                params![task_id, caller.owner, caller.desktop],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .context("agent_task_not_found")?;
        let target: Scope = serde_json::from_str(&key)?;
        let report: Option<(String, String)> = db
            .query_row(
                "SELECT id,value FROM events WHERE kind='agent_report' AND json_extract(value,'$.task_id')=?1
                 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3 ORDER BY seq DESC LIMIT 1",
                params![task_id, caller.owner, caller.desktop],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let report_id = report.as_ref().map(|(id, _)| id.as_str());
        let report = report
            .as_ref()
            .map(|(_, s)| serde_json::from_str::<Value>(s))
            .transpose()?;
        let record_id = report.as_ref().and_then(|r| r["result_record_id"].as_str());
        let result: Option<String> = if let Some(record_id) = record_id {
            db.query_row(
                "SELECT json_extract(value,'$.text') FROM events WHERE scope=?1 AND id=?2 AND kind='assistant'",
                params![key, record_id],
                |r| r.get(0),
            ).optional()?
        } else {
            None
        };
        let truncated = result.as_ref().is_some_and(|s| s.len() > limit);
        let result = result.map(|mut text| {
            text.truncate(text.floor_char_boundary(limit.min(text.len())));
            text
        });
        let done = !matches!(state.as_str(), "running" | "stopping" | "finishing");
        if let Some(run) = pin_run {
            let active: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND scope=?2 AND state='running')",
                params![run, caller.key()?],
                |r| r.get(0),
            )?;
            ensure!(active, "run_not_active");
            // Keep the lookup and pinning under the same lock as clean(), including the
            // report that carries the durable link to the answer and error information.
            db.execute(
                "INSERT OR IGNORE INTO pins(run,event) SELECT ?1,seq FROM events WHERE id=?2 OR id=?3",
                params![run, report_id, record_id],
            )?;
        }
        let value = json!({
            "task_id":task_id,"run_id":task_id,"agent_id":target.agent,"session_id":target.session,
            "root_user_message_id":root,"state":state,"done":done,
            "result_available":result.is_some(),"result_text":result,
            "result_record_id":if result.is_some(){record_id}else{None},"result_truncated":truncated,
            "error":report.as_ref().and_then(|r|r.get("error")),
            "error_truncated":report.as_ref().is_some_and(|r| r["error_truncated"] == true),
            "outcome_available":report.is_some()
        });
        Ok((target, value))
    }
    pub fn events_after(&self, scope: &Scope, after: i64) -> Result<Vec<HistoryItem>> {
        let db = self.db.lock().unwrap();
        let mut query=db.prepare("SELECT seq,id,kind,root,at,value FROM events WHERE scope=?1 AND seq>?2 ORDER BY seq LIMIT 128")?;
        let rows = query
            .query_map(params![scope.key()?, after], decode_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    pub fn generation(&self, scope: &Scope) -> Result<i64> {
        let db = self.db.lock().unwrap();
        ensure_scope(&db, &scope.key()?)?;
        Ok(db.query_row(
            "SELECT generation FROM scopes WHERE scope=?1",
            [scope.key()?],
            |r| r.get(0),
        )?)
    }
    pub fn pending_actions(&self, scope: &Scope) -> Result<Vec<Value>> {
        let db = self.db.lock().unwrap();
        let mut query=db.prepare("SELECT id,state FROM actions WHERE scope=?1 AND state IN ('prepared','accepted','unknown') ORDER BY rowid DESC LIMIT 128")?;
        Ok(query
            .query_map([scope.key()?], |r| {
                Ok(json!({"action_id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?}))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Uploads expire after 24 hours; private scoped staging never becomes model text.
    pub fn image_begin(&self, scope: &Scope, mime: &str, size: usize) -> Result<String> {
        ensure!(
            matches!(
                mime,
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            ),
            "unsupported_image_type"
        );
        ensure!(size > 0 && size <= 4 * 1024 * 1024, "image_size_limit");
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute(
            "DELETE FROM image_uploads WHERE created<?1",
            [now() - 86_400_000],
        )?;
        let staged: i64 =
            tx.query_row("SELECT COALESCE(SUM(size),0) FROM image_uploads", [], |r| {
                r.get(0)
            })?;
        ensure!(
            staged + size as i64 <= 128 * 1024 * 1024,
            "image_staging_full"
        );
        let token = id();
        tx.execute(
            "INSERT INTO image_uploads VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                token,
                scope.key()?,
                mime,
                size as i64,
                Vec::<u8>::new(),
                now()
            ],
        )?;
        tx.commit()?;
        Ok(token)
    }
    pub fn image_release(&self, scope: &Scope, token: &str) -> Result<()> {
        self.db.lock().unwrap().execute(
            "DELETE FROM image_uploads WHERE id=?1 AND scope=?2",
            params![token, scope.key()?],
        )?;
        Ok(())
    }
    pub fn image_chunk(
        &self,
        scope: &Scope,
        token: &str,
        offset: usize,
        chunk: &[u8],
    ) -> Result<usize> {
        ensure!(
            !chunk.is_empty() && chunk.len() <= 32768,
            "image_chunk_limit"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let (size, mut bytes): (i64, Vec<u8>) = tx
            .query_row(
                "SELECT size,body FROM image_uploads WHERE id=?1 AND scope=?2",
                params![token, scope.key()?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("image_upload_not_found")?;
        ensure!(
            offset <= bytes.len()
                && offset
                    .checked_add(chunk.len())
                    .is_some_and(|end| end as i64 <= size),
            "image_offset_invalid"
        );
        if offset < bytes.len() {
            ensure!(
                bytes.get(offset..offset + chunk.len()) == Some(chunk),
                "image_chunk_conflict"
            );
        } else {
            bytes.extend_from_slice(chunk);
            tx.execute(
                "UPDATE image_uploads SET body=?2 WHERE id=?1",
                params![token, bytes],
            )?;
        }
        tx.commit()?;
        Ok(bytes.len())
    }
    pub fn record_bytes(&self, scope: &Scope, record_id: &str) -> Result<(Record, Vec<u8>)> {
        let db = self.db.lock().unwrap();
        let target = accessible_record_scope(&db, scope, record_id)?;
        let record = read_record(&db, &target.key()?, record_id)?;
        let bytes=db.query_row("SELECT b.body FROM records r JOIN blobs b ON r.hash=b.hash WHERE r.scope=?1 AND r.id=?2",params![target.key()?,record_id],|r|r.get::<_,Vec<u8>>(0))?;
        let hash: String =
            db.query_row("SELECT hash FROM records WHERE id=?1", [record_id], |r| {
                r.get(0)
            })?;
        ensure!(
            blake3::hash(&bytes).to_hex().as_str() == hash,
            "record_integrity_error"
        );
        Ok((record, bytes))
    }
    pub fn history_index(&self, scope: &Scope, run: &str, high: i64) -> Result<Record> {
        self.archive(
            scope,
            run,
            &format!("history-index-{high}"),
            "history_index",
            json!({"high":high,"generation":self.generation(scope)?}),
            &serde_json::to_vec(&json!({"high":high}))?,
        )
    }
    pub fn latest_sequence(&self, scope: &Scope) -> Result<i64> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row(
            "SELECT COALESCE(MAX(seq),0) FROM events WHERE scope=?1",
            [scope.key()?],
            |r| r.get(0),
        )?)
    }
    pub fn record_page(
        &self,
        scope: &Scope,
        record_id: &str,
        part: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        ensure!((4..=16384).contains(&limit), "invalid_record_page_size");
        let db = self.db.lock().unwrap();
        materialize_event(&db, scope, record_id)?;
        let target = accessible_record_scope(&db, scope, record_id)?;
        let key = target.key()?;
        let record = read_record(&db, &key, record_id)?;
        if record.kind == "history_index" && part == "body" {
            let generation = self.generation_unlocked(&db, &key)?;
            ensure!(record.metadata["generation"] == generation, "index_expired");
            let kind = format!("history_index/{record_id}");
            let high = record.metadata["high"].as_i64().context("invalid_index")?;
            let before = if let Some(cursor) = cursor {
                let c: Cursor = self.decode(cursor)?;
                ensure!(
                    c.scope == scope.key()? && c.kind == kind && c.generation == generation,
                    "cursor_scope_mismatch"
                );
                c.before
            } else {
                high + 1
            };
            let mut query=db.prepare("SELECT seq,id,kind,at FROM events WHERE scope=?1 AND seq<?2 AND seq<=?3 AND kind NOT IN ('analysis','unit_update','archive_index') ORDER BY seq DESC LIMIT 51")?;
            let mut rows=query.query_map(params![key,before,high],|r|Ok(json!({"sequence":r.get::<_,i64>(0)?,"record_id":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"created_at":r.get::<_,i64>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let more = rows.len() > 50;
            rows.truncate(50);
            let cursor = if more {
                Some(self.encode(&Cursor {
                    scope: scope.key()?,
                    kind,
                    generation,
                    before: rows.last().unwrap()["sequence"].as_i64().unwrap(),
                    high,
                })?)
            } else {
                None
            };
            return Ok(
                json!({"record_id":record_id,"kind":"history_index","entries":rows,"cursor":cursor}),
            );
        }
        let generation: i64 = db.query_row(
            "SELECT generation FROM scopes WHERE scope=?1",
            [&key],
            |r| r.get(0),
        )?;
        let kind = format!("record/{record_id}/{part}");
        let before = if let Some(cursor) = cursor {
            let c: Cursor = self.decode(cursor)?;
            ensure!(
                c.scope == scope.key()? && c.kind == kind,
                "cursor_scope_mismatch"
            );
            ensure!(c.generation == generation, "cursor_expired");
            usize::try_from(c.before)?
        } else {
            0
        };
        if part != "body" {
            ensure!(before == 0, "invalid_record_cursor");
            return Ok(match part {
                "anchors" => record_anchors(&record),
                "summary" => json!({"record_id":record_id,"summary":record.summary}),
                _ => bail!("invalid_record_part"),
            });
        }
        ensure!(before <= record.bytes, "invalid_record_cursor");
        let mut bytes:Vec<u8>=db.query_row("SELECT substr(b.body,?3,?4) FROM records r JOIN blobs b ON r.hash=b.hash WHERE r.scope=?1 AND r.id=?2",params![key,record_id,(before+1) as i64,limit as i64],|r|r.get(0))?;
        let binary = record.kind == "png" || record.metadata["binary"] == true;
        let body = if binary {
            URL_SAFE_NO_PAD.encode(&bytes)
        } else {
            match std::str::from_utf8(&bytes) {
                Ok(text) => text.to_owned(),
                Err(error) if error.error_len().is_none() => {
                    bytes.truncate(error.valid_up_to());
                    std::str::from_utf8(&bytes)?.to_owned()
                }
                Err(error) => return Err(error.into()),
            }
        };
        ensure!(
            !bytes.is_empty() || before == record.bytes,
            "record_page_too_small"
        );
        let body_hash: String =
            db.query_row("SELECT hash FROM records WHERE id=?1", [record_id], |r| {
                r.get(0)
            })?;
        if before == 0 {
            let original: Vec<u8> =
                db.query_row("SELECT body FROM blobs WHERE hash=?1", [&body_hash], |r| {
                    r.get(0)
                })?;
            ensure!(
                blake3::hash(&original).to_hex().as_str() == body_hash,
                "record_integrity_error"
            );
        }
        let chunk_hash = blake3::hash(&bytes).to_hex().to_string();
        let next = before + bytes.len();
        let cursor = if next < record.bytes {
            Some(self.encode(&Cursor {
                scope: scope.key()?,
                kind,
                generation,
                before: next as i64,
                high: record.bytes as i64,
            })?)
        } else {
            None
        };
        Ok(
            json!({"record_id":record_id,"body":body,"body_hash":body_hash,"chunk_hash":chunk_hash,"encoding":if binary{"base64url"}else{"utf8"},"cursor":cursor,"offset":before,"total_bytes":record.bytes,"partial":next<record.bytes || before>0,"metadata":record.metadata,"kind":record.kind}),
        )
    }
    pub fn pin_context(
        &self,
        scope: &Scope,
        run: &str,
        generation: i64,
        units: &[String],
    ) -> Result<()> {
        ensure!(units.len() <= 4096, "context_unit_limit");
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let key = scope.key()?;
        let current: i64 = tx.query_row(
            "SELECT generation FROM scopes WHERE scope=?1",
            [&key],
            |r| r.get(0),
        )?;
        ensure!(current == generation, "history_changed_rebuild_context");
        for unit in units {
            tx.execute(
                "INSERT OR IGNORE INTO pins SELECT ?1,seq FROM events WHERE scope=?2 AND id=?3",
                params![run, key, unit],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn pin_record(&self, scope: &Scope, run: &str, record_id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        materialize_event(&db, scope, record_id)?;
        accessible_record_scope(&db, scope, record_id)?;
        let active: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND scope=?2 AND state='running')",
            params![run, scope.key()?],
            |r| r.get(0),
        )?;
        ensure!(active, "run_not_active");
        let mut queue = vec![record_id.to_owned()];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = queue.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            ensure!(seen.len() <= 512, "record_dependency_limit");
            let target = accessible_record_scope(&db, scope, &id)?;
            db.execute(
                "INSERT OR IGNORE INTO pins(run,event) SELECT ?1,event FROM records WHERE id=?2",
                params![run, id],
            )?;
            let record = read_record(&db, &target.key()?, &id)?;
            if record.kind == "context_index" {
                if let Some(units) = record.metadata["source_units"].as_array() {
                    for unit in units.iter().filter_map(Value::as_str) {
                        db.execute("INSERT OR IGNORE INTO pins SELECT ?1,seq FROM events WHERE scope=?2 AND id=?3",params![run,target.key()?,unit])?;
                    }
                }
                if let Some(records) = record.metadata["records"].as_array() {
                    queue.extend(records.iter().filter_map(Value::as_str).map(str::to_owned));
                }
            }
        }
        Ok(())
    }
    pub fn view_tui_lines(&self, scope: &Scope, view_id: &str) -> Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut query=db.prepare("SELECT summary FROM records WHERE scope=?1 AND json_extract(metadata,'$.view_id')=?2 AND summary IS NOT NULL ORDER BY rowid DESC LIMIT 32")?;
        let rows = query
            .query_map(params![scope.key()?, view_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut lines = std::collections::BTreeSet::new();
        for row in rows {
            let summary: Value = serde_json::from_str(&row)?;
            if let Some(values) = summary["tui_lines"].as_array() {
                lines.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
            }
        }
        ensure!(
            lines.len() <= 1000 && lines.iter().map(String::len).sum::<usize>() <= 64 * 1024,
            "tui_annotation_limit"
        );
        Ok(lines.into_iter().collect())
    }
    pub fn find_record(&self, scope: &Scope, request: &str) -> Result<Option<Record>> {
        record_by_request(&self.db.lock().unwrap(), &scope.key()?, request)
    }
    pub fn append_identified(
        &self,
        scope: &Scope,
        kind: &str,
        root: &str,
        event_id: &str,
        mut value: Value,
        pin_run: Option<&str>,
    ) -> Result<i64> {
        ensure!(
            ["assistant", "interaction", "agent_report"].contains(&kind),
            "invalid_event_kind"
        );
        if kind == "assistant"
            && let Some(run) = pin_run
        {
            value
                .as_object_mut()
                .context("invalid_assistant_event")?
                .insert("run_id".into(), json!(run));
        }
        let db = self.db.lock().unwrap();
        let key = scope.key()?;
        ensure_scope(&db, &key)?;
        db.execute(
            "INSERT INTO events(scope,id,kind,root,at,value) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                key,
                event_id,
                kind,
                root,
                now(),
                bounded_json(&value, 24 * 1024)?
            ],
        )?;
        let sequence = db.last_insert_rowid();
        if let Some(run) = pin_run {
            db.execute(
                "INSERT OR IGNORE INTO pins VALUES(?1,?2)",
                params![run, sequence],
            )?;
        }
        Ok(sequence)
    }
    pub fn unit_update(&self, scope: &Scope, unit: &str, value: Value) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let root: Option<String> = tx
            .query_row(
                "SELECT root FROM events WHERE scope=?1 AND id=?2",
                params![scope.key()?, unit],
                |r| r.get(0),
            )
            .context("interaction_not_found")?;
        tx.execute("UPDATE events SET at=?2 WHERE id=?1", params![unit, now()])?;
        tx.execute(
            "INSERT INTO unit_updates(unit,value) VALUES(?1,?2)",
            params![unit, bounded_json(&value, 16 * 1024)?],
        )?;
        insert_event(
            &tx,
            &scope.key()?,
            "unit_update",
            root.as_deref(),
            &json!({"unit_id":unit}).to_string(),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn live_units(
        &self,
        scope: &Scope,
        ids: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        let db = self.db.lock().unwrap();
        let mut live = std::collections::HashSet::new();
        let mut query =
            db.prepare("SELECT EXISTS(SELECT 1 FROM events WHERE scope=?1 AND id=?2)")?;
        for id in ids {
            if query.query_row(params![scope.key()?, id], |r| r.get::<_, bool>(0))? {
                live.insert(id.clone());
            }
        }
        Ok(live)
    }
    pub fn recent_events(&self, scope: &Scope) -> Result<Vec<HistoryItem>> {
        let db = self.db.lock().unwrap();
        let mut query=db.prepare("SELECT seq,id,kind,root,at,value FROM events WHERE scope=?1 AND kind NOT IN ('analysis','unit_update','archive_index') ORDER BY seq DESC LIMIT 128")?;
        let mut rows = query
            .query_map([scope.key()?], decode_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }
    pub fn append(
        &self,
        scope: &Scope,
        kind: &str,
        root: Option<&str>,
        value: Value,
    ) -> Result<i64> {
        ensure!(
            [
                "assistant",
                "interaction",
                "agent_report",
                "analysis",
                "history_pruned"
            ]
            .contains(&kind),
            "invalid_event_kind"
        );
        let db = self.db.lock().unwrap();
        let key = scope.key()?;
        ensure_scope(&db, &key)?;
        insert_event(&db, &key, kind, root, &bounded_json(&value, 24 * 1024)?)
    }
    pub fn model_usage(
        &self,
        scope: &Scope,
        run: &str,
        root: &str,
        stage: &str,
        value: Value,
    ) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT INTO model_usage VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id(),
                scope.key()?,
                run,
                root,
                stage,
                bounded_json(&value, 16384)?
            ],
        )?;
        Ok(())
    }
    pub fn finish_run(&self, scope: &Scope, run: &str, state: &str) -> Result<()> {
        self.finish_run_with_error(scope, run, state, None)
            .map(|_| ())
    }
    pub fn finish_run_with_error(
        &self,
        scope: &Scope,
        run: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<Value> {
        ensure!(
            ["completed", "cancelled", "failed", "paused", "orphaned"].contains(&state),
            "invalid_run_state"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let count = tx.execute(
            "UPDATE runs SET state=?3 WHERE id=?1 AND scope=?2",
            params![run, scope.key()?, state],
        )?;
        ensure!(count == 1, "run_not_found");
        let result_record_id: Option<String> = tx.query_row(
            "SELECT id FROM events WHERE scope=?1 AND kind='assistant' AND json_extract(value,'$.run_id')=?2 ORDER BY seq DESC LIMIT 1",
            params![scope.key()?, run], |r|r.get(0)
        ).optional()?;
        let (error, error_truncated) = task_error(error);
        let report = json!({"task_id":run,"agent_id":scope.agent,"session_id":scope.session,
            "state":state,"error":error,"error_truncated":error_truncated,"result_record_id":result_record_id});
        let delegated: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM delegations WHERE run=?1 AND scope=?2)",
            params![run, scope.key()?],
            |r| r.get(0),
        )?;
        if delegated {
            let root: String =
                tx.query_row("SELECT root FROM runs WHERE id=?1", [run], |r| r.get(0))?;
            insert_event(
                &tx,
                &scope.key()?,
                "agent_report",
                Some(&root),
                &bounded_json(&report, 24 * 1024)?,
            )?;
        }
        tx.execute("DELETE FROM pins WHERE run=?1", [run])?;
        tx.commit()?;
        Ok(report)
    }
    pub fn prepare_action(
        &self,
        scope: &Scope,
        run: &str,
        action: &str,
        payload: &Value,
    ) -> Result<Action> {
        let key = scope.key()?;
        let db = self.db.lock().unwrap();
        let state: String = db
            .query_row(
                "SELECT state FROM runs WHERE id=?1 AND scope=?2",
                params![run, key],
                |r| r.get(0),
            )
            .context("run_not_found")?;
        ensure!(state == "running", "run_not_active");
        let hash = blake3::hash(bounded_json(payload, 64 * 1024)?.as_bytes())
            .to_hex()
            .to_string();
        if let Some((old, state)) = db
            .query_row(
                "SELECT hash,state FROM actions WHERE scope=?1 AND id=?2",
                params![key, action],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure!(old == hash, "action_payload_conflict");
            return Ok(Action {
                id: action.into(),
                state,
                duplicate: true,
            });
        }
        db.execute(
            "INSERT INTO actions VALUES(?1,?2,?3,?4,'prepared')",
            params![key, action, run, hash],
        )?;
        Ok(Action {
            id: action.into(),
            state: "prepared".into(),
            duplicate: false,
        })
    }
    pub fn action_receipt(&self, scope: &Scope, action: &str, state: &str) -> Result<()> {
        ensure!(
            ["accepted", "written", "failed", "unknown"].contains(&state),
            "invalid_action_receipt"
        );
        let db = self.db.lock().unwrap();
        ensure!(db.execute("UPDATE actions SET state=?3 WHERE scope=?1 AND id=?2 AND state IN ('prepared','accepted')",params![scope.key()?,action,state])?==1,"action_not_pending");
        Ok(())
    }
    pub fn archive(
        &self,
        scope: &Scope,
        run: &str,
        request: &str,
        kind: &str,
        metadata: Value,
        payload: &[u8],
    ) -> Result<Record> {
        ensure!(payload.len() <= 4 * 1024 * 1024, "record_size_limit");
        let key = scope.key()?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        ensure_scope(&tx, &key)?;
        if let Some(record) = record_by_request(&tx, &key, request)? {
            let hash: String =
                tx.query_row("SELECT hash FROM records WHERE id=?1", [&record.id], |r| {
                    r.get(0)
                })?;
            ensure!(
                hash == blake3::hash(payload).to_hex().as_str()
                    && record.kind == kind
                    && record.metadata == metadata,
                "record_request_conflict"
            );
            return Ok(record);
        }
        let root: String = tx
            .query_row(
                "SELECT root FROM runs WHERE id=?1 AND scope=?2 AND state='running'",
                params![run, key],
                |r| r.get(0),
            )
            .context("run_not_active")?;
        let record_id = id();
        let hash = blake3::hash(payload).to_hex().to_string();
        let meta = bounded_json(&metadata, 32 * 1024)?;
        let event = if let Some(unit) = metadata["history_unit_id"].as_str() {
            tx.query_row(
                "SELECT seq FROM events WHERE scope=?1 AND id=?2",
                params![key, unit],
                |r| r.get::<_, i64>(0),
            )
            .context("interaction_not_found")?
        } else {
            insert_event(&tx,&key,if matches!(kind,"context_index"|"history_index"){"archive_index"}else{"interaction"},Some(&root),&json!({"record_id":record_id,"kind":kind,"bytes":payload.len(),"pending":!matches!(kind,"context_index"|"history_index"|"associated_text")}).to_string())?
        };
        tx.execute(
            "INSERT OR IGNORE INTO blobs VALUES(?1,?2)",
            params![hash, payload],
        )?;
        tx.execute(
            "INSERT INTO records VALUES(?1,?2,?3,?4,?5,?6,NULL,?8,?7)",
            params![
                record_id,
                key,
                request,
                kind,
                hash,
                meta,
                event,
                !matches!(kind, "context_index" | "history_index" | "associated_text")
            ],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO pins VALUES(?1,?2)",
            params![run, event],
        )?;
        tx.commit()?;
        Ok(Record {
            id: record_id,
            kind: kind.into(),
            metadata,
            summary: None,
            bytes: payload.len(),
            pending: !matches!(kind, "context_index" | "history_index" | "associated_text"),
        })
    }
    pub fn analyze(&self, scope: &Scope, record_id: &str, digest: Value) -> Result<Value> {
        self.analyze_observation(scope, record_id, digest, true)
    }
    pub fn analyze_observation(
        &self,
        scope: &Scope,
        record_id: &str,
        digest: Value,
        full_body: bool,
    ) -> Result<Value> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let target = accessible_record_scope(&tx, scope, record_id)?;
        let (body,metadata,kind):(Vec<u8>,String,String)=tx.query_row("SELECT b.body,r.metadata,r.kind FROM records r JOIN blobs b ON b.hash=r.hash WHERE r.scope=?1 AND r.id=?2",params![target.key()?,record_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).context("record_expired")?;
        let meta: Value = serde_json::from_str(&metadata)?;
        let body = if kind == "png" || meta["binary"] == true {
            meta["text"].as_str().unwrap_or("")
        } else {
            std::str::from_utf8(&body)?
        };
        let mut digest = digest;
        ensure!(
            digest["summary"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty()),
            "analysis_summary_required"
        );
        if !digest["key_quotes"].is_null() {
            let quotes = digest["key_quotes"]
                .as_array_mut()
                .context("invalid_analysis_quotes")?;
            for quote in quotes {
                ensure!(
                    quote["record_id"] == record_id
                        && quote["text"]
                            .as_str()
                            .is_some_and(|q| !q.is_empty() && body.contains(q)),
                    "invalid_analysis_quote"
                );
                let hash = blake3::hash(
                    format!("{record_id}:{}", quote["text"].as_str().unwrap()).as_bytes(),
                );
                quote["quote_id"] = json!(&hash.to_hex().as_str()[..16]);
            }
        }
        let metadata: Value = serde_json::from_str(&metadata)?;
        digest["record_id"] = json!(record_id);
        if let Some(facts) = digest.get("facts").filter(|v| !v.is_null()) {
            let facts = facts.as_array().context("invalid_analysis_facts")?;
            ensure!(facts.len() <= 64, "analysis_fact_limit");
            for fact in facts {
                ensure!(
                    fact["claim"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 4096),
                    "invalid_analysis_claim"
                );
                let evidence = &fact["evidence"];
                let valid = if let Some(text) = evidence.as_str() {
                    !text.is_empty() && body.contains(text)
                } else {
                    evidence["record_id"] == record_id
                        && evidence["text"]
                            .as_str()
                            .is_some_and(|text| !text.is_empty() && body.contains(text))
                };
                ensure!(
                    valid || fact["certainty"] == "unknown",
                    "unverified_analysis_evidence"
                );
            }
        }
        if kind == "text" && metadata["alternate_screen"] != true {
            ensure!(
                digest["tui_lines"].is_array(),
                "analysis_tui_classification_required"
            );
        }
        let mut tui_lines = std::collections::BTreeSet::<String>::new();
        if let Some(lines) = digest.get("tui_lines").filter(|v| !v.is_null()) {
            let lines = lines.as_array().context("invalid_analysis_tui_lines")?;
            ensure!(lines.len() <= 256, "tui_annotation_limit");
            for line in lines {
                let line = line.as_str().context("invalid_analysis_tui_line")?;
                ensure!(
                    !line.trim().is_empty() && body.split('\n').any(|original| original == line),
                    "invalid_analysis_tui_line"
                );
                tui_lines.insert(line.to_owned());
            }
        }
        if let Some(lines) = metadata["anchor_tui_lines"].as_array() {
            for line in lines.iter().filter_map(Value::as_str) {
                if !line.trim().is_empty() && body.split('\n').any(|original| original == line) {
                    tui_lines.insert(line.to_owned());
                }
            }
        }
        let previous: Option<String> = tx.query_row(
            "SELECT summary FROM records WHERE id=?1",
            [record_id],
            |r| r.get(0),
        )?;
        let mut classification_complete = full_body;
        if let Some(previous) = previous {
            let previous: Value = serde_json::from_str(&previous)?;
            classification_complete |= previous["tui_classification_complete"] == true;
            if let Some(lines) = previous["tui_lines"].as_array() {
                for line in lines.iter().filter_map(Value::as_str) {
                    if !line.trim().is_empty() && body.split('\n').any(|original| original == line)
                    {
                        tui_lines.insert(line.to_owned());
                    }
                }
            }
        }
        ensure!(
            tui_lines.len() <= 256 && tui_lines.iter().map(String::len).sum::<usize>() <= 16 * 1024,
            "tui_annotation_limit"
        );
        digest["tui_lines"] = json!(tui_lines);
        if kind == "text" {
            digest["tui_classification_complete"] = json!(classification_complete);
            let (head, tail, status) = if classification_complete {
                search_anchors(body, &metadata, &tui_lines)
            } else {
                (
                    json!({"lines":[],"positions":[]}),
                    json!({"lines":[],"positions":[]}),
                    "unclassified_partial_body",
                )
            };
            digest["search_head_anchor"] = head;
            digest["search_tail_anchor"] = tail;
            digest["search_anchor_status"] = json!(status);
        }
        digest["head_anchor"] = metadata["head"].clone();
        digest["tail_anchor"] = metadata["tail"].clone();
        // Analysis cannot promote a TUI observation into verified process completion.
        digest["observed_status"] = metadata
            .get("observed_status")
            .cloned()
            .unwrap_or_else(|| json!({"state":"unknown","evidence_source":"terminal_observation"}));
        let summary = bounded_json(&digest, 32 * 1024)?;
        tx.execute(
            "INSERT INTO analyses VALUES(?1,?2,?3,?4,?5)",
            params![id(), target.key()?, record_id, now(), summary],
        )?;
        tx.execute(
            "UPDATE records SET summary=?3,pending=0 WHERE scope=?1 AND id=?2",
            params![target.key()?, record_id, summary],
        )?;
        tx.commit()?;
        Ok(digest)
    }
    pub fn record(
        &self,
        scope: &Scope,
        record_id: &str,
        part: &str,
        offset: usize,
    ) -> Result<RecordPage> {
        let db = self.db.lock().unwrap();
        let target = accessible_record_scope(&db, scope, record_id)?;
        let record = read_record(&db, &target.key()?, record_id)?;
        let bytes:Vec<u8>=match part {
            "anchors"=>serde_json::to_vec(&record_anchors(&record))?,
            "summary"=>serde_json::to_vec(&record.summary)?,
            "body"=>db.query_row("SELECT substr(b.body,?3,?4) FROM blobs b JOIN records r ON r.hash=b.hash WHERE r.scope=?1 AND r.id=?2",params![target.key()?,record_id,i64::try_from(offset)?.checked_add(1).context("invalid_record_offset")?,12*1024],|r|r.get(0))?,
            _=>bail!("invalid_record_part"),
        };
        if part != "body" {
            ensure!(offset == 0, "invalid_record_offset");
        }
        ensure!(offset <= record.bytes, "invalid_record_offset");
        let next_offset = if part == "body" && offset + bytes.len() < record.bytes {
            Some(offset + bytes.len())
        } else {
            None
        };
        Ok(RecordPage {
            record,
            payload: URL_SAFE_NO_PAD.encode(bytes),
            encoding: "base64url".into(),
            next_offset,
        })
    }
    pub fn history(&self, scope: &Scope, cursor: Option<&str>) -> Result<HistoryPage> {
        let key = scope.key()?;
        let db = self.db.lock().unwrap();
        ensure_scope(&db, &key)?;
        let generation: i64 = db.query_row(
            "SELECT generation FROM scopes WHERE scope=?1",
            [&key],
            |r| r.get(0),
        )?;
        let c = if let Some(c) = cursor {
            let c: Cursor = self.decode(c)?;
            ensure!(
                c.scope == key && c.kind == "history",
                "cursor_scope_mismatch"
            );
            ensure!(c.generation == generation, "cursor_expired");
            c
        } else {
            let high: i64 = db.query_row(
                "SELECT COALESCE(MAX(seq),0) FROM events WHERE scope=?1",
                [&key],
                |r| r.get(0),
            )?;
            Cursor {
                scope: key.clone(),
                kind: "history".into(),
                generation,
                before: high + 1,
                high,
            }
        };
        let mut statement=db.prepare("SELECT seq,id,kind,root,at,value FROM events WHERE scope=?1 AND seq<?2 AND seq<=?3 AND kind NOT IN ('analysis','unit_update','archive_index') ORDER BY seq DESC LIMIT 51")?;
        let mut items = statement
            .query_map(params![key, c.before, c.high], |r| {
                Ok(HistoryItem {
                    sequence: r.get(0)?,
                    id: r.get(1)?,
                    kind: r.get(2)?,
                    root_user_message_id: r.get(3)?,
                    created_at: r.get(4)?,
                    value: serde_json::from_str(&r.get::<_, String>(5)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = items.len() > 50;
        items.truncate(50);
        for item in &mut items {
            if item.kind == "interaction" {
                let updates = unit_updates(&db, &item.id)?;
                item.value["updates"] = json!(updates);
                item.value["records"] = history_records(&db, item.sequence)?;
            }
            if serde_json::to_vec(&item.value)?.len() > 12 * 1024 {
                let record_id = if item.kind == "interaction" {
                    history_snapshot(&db, &key, item)?
                } else {
                    item.id.clone()
                };
                let text = item.value["text"]
                    .as_str()
                    .or_else(|| item.value["message"].as_str())
                    .or_else(|| item.value["summary"].as_str())
                    .unwrap_or("Large history item; open the original record for details.");
                let mut end = text.len().min(8000);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                item.value = json!({"text":&text[..end],"record_id":record_id,"partial":true});
            }
        }
        let next = if has_more {
            Some(self.encode(&Cursor {
                before: items.last().unwrap().sequence,
                ..c
            })?)
        } else {
            None
        };
        Ok(HistoryPage {
            snapshot_watermark: c.high,
            items,
            cursor: next,
            has_more,
            generation,
        })
    }
    pub fn projection(&self, scope: &Scope) -> Result<Option<(i64, i64, Value)>> {
        let db = self.db.lock().unwrap();
        let row: Option<(i64, i64, String)> = db
            .query_row(
                "SELECT generation,covered,value FROM projections WHERE scope=?1",
                [scope.key()?],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(g, c, s)| Ok((g, c, serde_json::from_str(&s)?)))
            .transpose()
    }
    pub fn checkpoint(
        &self,
        scope: &Scope,
        generation: i64,
        covered: i64,
        value: &Value,
    ) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT INTO projections VALUES(?1,?2,?3,?4) ON CONFLICT(scope) DO UPDATE SET generation=excluded.generation,covered=excluded.covered,value=excluded.value",params![scope.key()?,generation,covered,bounded_json(value,1024*1024)?])?;
        Ok(())
    }

    pub fn owner_retention(
        &self,
        owner: &str,
        desktop: &str,
        change: Option<Option<Retention>>,
    ) -> Result<Option<Retention>> {
        let db = self.db.lock().unwrap();
        if let Some(change) = change {
            if let Some(rule) = &change {
                validate_retention(rule)?;
            }
            db.execute("INSERT INTO owner_retention VALUES(?1,?2,?3) ON CONFLICT(owner,desktop) DO UPDATE SET rule=excluded.rule",params![owner,desktop,change.map(|r|serde_json::to_string(&r)).transpose()?])?;
        }
        let value: Option<String> = db
            .query_row(
                "SELECT rule FROM owner_retention WHERE owner=?1 AND desktop=?2",
                params![owner, desktop],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        value.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
    }
    pub fn storage_pressure(&self) -> Result<bool> {
        let db = self.db.lock().unwrap();
        let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let free: i64 = db.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
        let size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok((pages - free) * size > 1536 * 1024 * 1024)
    }
    pub fn retention(
        &self,
        scope: &Scope,
        rule: Option<Option<Retention>>,
    ) -> Result<Option<Retention>> {
        let db = self.db.lock().unwrap();
        let key = scope.key()?;
        ensure_scope(&db, &key)?;
        if let Some(rule) = rule {
            if let Some(rule) = &rule {
                validate_retention(rule)?;
            }
            db.execute(
                "UPDATE scopes SET retention=?2 WHERE scope=?1",
                params![key, serde_json::to_string(&rule)?],
            )?;
        }
        let value: Option<String> =
            db.query_row("SELECT retention FROM scopes WHERE scope=?1", [key], |r| {
                r.get(0)
            })?;
        if let Some(value) = value {
            return Ok(serde_json::from_str::<Option<Retention>>(&value)?);
        }
        drop(db);
        self.owner_retention(&scope.owner, &scope.desktop, None)
    }
    pub fn clean(&self, scope: &Scope, rule: &Retention, dry_run: bool) -> Result<CleanResult> {
        validate_retention(rule)?;
        let key = scope.key()?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        ensure_scope(&tx, &key)?;
        let fingerprint = serde_json::to_string(rule)?;
        let saved: Option<(String, i64, i64, i64)> = tx
            .query_row(
                "SELECT rule,high,bound,after FROM clean_pass WHERE scope=?1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (high, bound, after) = if let Some((old, high, bound, after)) =
            saved.filter(|r| r.0 == fingerprint && !dry_run)
        {
            let _ = old;
            (high, bound, after)
        } else {
            let high: i64 = tx.query_row(
                "SELECT COALESCE(MAX(seq),0) FROM events WHERE scope=?1",
                [&key],
                |r| r.get(0),
            )?;
            let bound=match rule {
                Retention::KeepLast{count}=>tx.query_row("SELECT COALESCE((SELECT seq FROM events WHERE scope=?1 AND seq<=?2 AND kind NOT IN ('analysis','unit_update','archive_index') ORDER BY seq DESC LIMIT 1 OFFSET ?3),0)",params![key,high,i64::try_from(*count)?-1],|r|r.get(0))?,
                Retention::Before{utc_ms}=>*utc_ms,
                Retention::OlderThan{days}=>now()-i64::from(*days)*86400000,
            };
            (high, bound, 0)
        };
        let condition = if matches!(rule, Retention::KeepLast { .. }) {
            "seq<?3"
        } else {
            "at<?3"
        };
        let filter = format!(
            "scope=?1 AND seq<=?2 AND {condition} AND kind NOT IN ('analysis','unit_update','archive_index')"
        );
        if dry_run {
            let (candidates,pinned):(i64,i64)=tx.query_row(&format!("SELECT COUNT(*),COALESCE(SUM(EXISTS(SELECT 1 FROM pins WHERE event=events.seq)),0) FROM events WHERE {filter}"),params![key,high,bound],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let generation = tx.query_row(
                "SELECT generation FROM scopes WHERE scope=?1",
                [&key],
                |r| r.get(0),
            )?;
            return Ok(CleanResult {
                scope: scope.clone(),
                cutoff_utc_ms: if matches!(rule, Retention::KeepLast { .. }) {
                    None
                } else {
                    Some(bound)
                },
                logical_reclaimed_bytes: 0,
                reusable_bytes: 0,
                candidates: candidates as u64,
                deleted: 0,
                pinned: pinned as u64,
                generation,
            });
        }
        let candidates = {
            let mut q = tx.prepare(&format!(
                "SELECT seq FROM events WHERE {filter} AND seq>?4 ORDER BY seq LIMIT 256"
            ))?;
            q.query_map(params![key, high, bound, after], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        if candidates.len() < 256 {
            tx.execute("DELETE FROM clean_pass WHERE scope=?1", [&key])?;
        } else {
            tx.execute(
                "INSERT OR REPLACE INTO clean_pass VALUES(?1,?2,?3,?4,?5)",
                params![key, fingerprint, high, bound, candidates.last().unwrap()],
            )?;
        }
        let mut deleted = 0;
        let mut pinned = 0;
        for seq in &candidates {
            let active: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM pins WHERE event=?1)",
                [seq],
                |r| r.get(0),
            )?;
            if active {
                pinned += 1;
            } else if !dry_run {
                deleted += tx.execute("DELETE FROM events WHERE seq=?1", [seq])?;
            }
        }
        let mut reclaimed = 0_i64;
        if deleted > 0 {
            tx.execute("DELETE FROM events WHERE scope=?1 AND seq IN (SELECT event FROM records WHERE scope=?1 AND kind IN ('context_index','history_index')) AND seq NOT IN (SELECT event FROM pins)",[&key])?;
            tx.execute("DELETE FROM model_usage WHERE scope=?1 AND root NOT IN (SELECT root FROM events WHERE scope=?1 AND root IS NOT NULL)",[&key])?;
            reclaimed=tx.query_row("SELECT COALESCE(SUM(length(body)),0) FROM blobs WHERE hash NOT IN (SELECT hash FROM records)",[],|r|r.get(0))?;
            tx.execute(
                "DELETE FROM blobs WHERE hash NOT IN (SELECT hash FROM records)",
                [],
            )?;
            tx.execute(
                "UPDATE scopes SET generation=generation+1 WHERE scope=?1",
                [&key],
            )?;
            tx.execute(
                "DELETE FROM analyses WHERE scope=?1 AND record NOT IN (SELECT id FROM records)",
                [&key],
            )?;
            tx.execute("DELETE FROM projections WHERE scope=?1", [&key])?;
        }
        let generation = tx.query_row(
            "SELECT generation FROM scopes WHERE scope=?1",
            [&key],
            |r| r.get(0),
        )?;
        let free: i64 = tx.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
        let size: i64 = tx.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        tx.commit()?;
        Ok(CleanResult {
            scope: scope.clone(),
            cutoff_utc_ms: if matches!(rule, Retention::KeepLast { .. }) {
                None
            } else {
                Some(bound)
            },
            logical_reclaimed_bytes: reclaimed as u64,
            reusable_bytes: (free * size) as u64,
            candidates: candidates.len() as u64,
            deleted: deleted as u64,
            pinned,
            generation,
        })
    }
    pub fn maintain(&self) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute_batch("PRAGMA wal_checkpoint(PASSIVE); PRAGMA incremental_vacuum(128);")?;
        Ok(())
    }
    fn encode(&self, cursor: &Cursor) -> Result<String> {
        let bytes = serde_json::to_vec(cursor)?;
        let hash = blake3::keyed_hash(&self.cursor_key, &bytes);
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            hash.to_hex()
        ))
    }
    fn decode<T: serde::de::DeserializeOwned>(&self, cursor: &str) -> Result<T> {
        ensure!(cursor.len() <= 4096, "invalid_cursor");
        let (data, hash) = cursor.split_once('.').context("invalid_cursor")?;
        let data = URL_SAFE_NO_PAD.decode(data)?;
        ensure!(
            blake3::keyed_hash(&self.cursor_key, &data)
                .to_hex()
                .as_str()
                == hash,
            "invalid_cursor_signature"
        );
        Ok(serde_json::from_slice(&data)?)
    }
}
fn search_anchors(
    body: &str,
    metadata: &Value,
    tui_lines: &std::collections::BTreeSet<String>,
) -> (Value, Value, &'static str) {
    let empty = json!({"lines":[],"positions":[]});
    if metadata["alternate_screen"] == true {
        return (empty.clone(), empty, "unavailable_tui_only");
    }
    if !metadata["fragment"].is_null() {
        return (empty.clone(), empty, "unavailable_fragment");
    }
    let first = metadata["start"].as_u64().unwrap_or(0) as usize;
    let stable = body
        .split('\n')
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !tui_lines.contains(*line))
        .collect::<Vec<_>>();
    let anchor = |tail: bool| {
        let count = metadata[if tail { "tail_lines" } else { "head_lines" }]
            .as_u64()
            .unwrap_or(if tail { 20 } else { 10 })
            .clamp(1, 100) as usize;
        let selected = if tail {
            &stable[stable.len().saturating_sub(count)..]
        } else {
            &stable[..stable.len().min(count)]
        };
        json!({"lines":selected.iter().map(|(_,line)|line).collect::<Vec<_>>(),"positions":selected.iter().map(|(index,_)|first+index).collect::<Vec<_>>()})
    };
    let status = if stable.is_empty() {
        if body.split('\n').any(|line| !line.trim().is_empty()) {
            "unavailable_tui_only"
        } else {
            "unavailable_blank_only"
        }
    } else {
        "available"
    };
    (anchor(false), anchor(true), status)
}
fn record_anchors(record: &Record) -> Value {
    let summary = record.summary.as_ref();
    let empty = json!({"lines":[],"positions":[]});
    let get = |key: &str| {
        summary
            .and_then(|s| s.get(key))
            .cloned()
            .unwrap_or_else(|| empty.clone())
    };
    let status = summary
        .and_then(|s| s.get("search_anchor_status"))
        .cloned()
        .unwrap_or_else(|| {
            json!(if record.metadata["alternate_screen"] == true {
                "unavailable_tui_only"
            } else {
                "unclassified"
            })
        });
    json!({"record_id":record.id,"head_anchor":record.metadata["head"],"tail_anchor":record.metadata["tail"],"search_head_anchor":get("search_head_anchor"),"search_tail_anchor":get("search_tail_anchor"),"tui_lines":summary.and_then(|s|s.get("tui_lines")).cloned().unwrap_or_else(||json!([])),"search_anchor_status":status,"metadata":record.metadata})
}
fn unit_updates(db: &Connection, unit: &str) -> Result<Vec<Value>> {
    let mut q =
        db.prepare("SELECT value FROM unit_updates WHERE unit=?1 ORDER BY seq DESC LIMIT 16")?;
    let rows = q
        .query_map([unit], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .rev()
        .map(|s| Ok(serde_json::from_str(&s)?))
        .collect()
}
/// The bounded update tail is not a complete index of a multi-tool interaction's records.
fn history_records(db: &Connection, sequence: i64) -> Result<Value> {
    let mut query = db.prepare("SELECT id,kind,json_extract(metadata,'$.source') FROM records WHERE event=?1 AND kind<>'history_event' ORDER BY rowid LIMIT 1024")?;
    Ok(json!(query.query_map([sequence], |row| Ok(json!({
        "record_id":row.get::<_, String>(0)?, "kind":row.get::<_, String>(1)?, "source":row.get::<_, Option<String>>(2)?
    })))?.collect::<rusqlite::Result<Vec<_>>>()?))
}
/// Interaction updates are mutable; each advertised full-body UUID must remain immutable.
fn history_snapshot(db: &Connection, scope: &str, item: &HistoryItem) -> Result<String> {
    let version: i64 = db.query_row(
        "SELECT coalesce(max(seq),0) FROM unit_updates WHERE unit=?1",
        [&item.id],
        |row| row.get(0),
    )?;
    let request = format!("history_snapshot/{}/{version}", item.id);
    let existing: Option<String> = db
        .query_row(
            "SELECT id FROM records WHERE scope=?1 AND request=?2",
            params![scope, request],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let body = bounded_json(&item.value, 4 * 1024 * 1024)?;
    let hash = blake3::hash(body.as_bytes()).to_hex().to_string();
    let record_id = id();
    let tx = db.unchecked_transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO blobs VALUES(?1,?2)",
        params![hash, body.as_bytes()],
    )?;
    tx.execute("INSERT INTO records VALUES(?1,?2,?3,'history_event',?4,?5,NULL,0,?6)",
        params![record_id, scope, request, hash, json!({"source":"history_event","event_kind":item.kind,"history_unit_id":item.id,"update_sequence":version}).to_string(), item.sequence])?;
    tx.commit()?;
    Ok(record_id)
}
fn materialize_event(db: &Connection, requester: &Scope, record_id: &str) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM records WHERE id=?1)",
        [record_id],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(());
    }
    let (scope, seq, kind, value): (String, i64, String, String) = db
        .query_row(
            "SELECT scope,seq,kind,value FROM events WHERE id=?1",
            [record_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .context("record_expired")?;
    let source: Scope = serde_json::from_str(&scope)?;
    ensure!(
        source.owner == requester.owner
            && source.desktop == requester.desktop
            && (requester.session.is_none() || requester.session == source.session),
        "record_forbidden"
    );
    let value = if kind == "interaction" {
        let mut value: Value = serde_json::from_str(&value)?;
        value["updates"] = json!(unit_updates(db, record_id)?);
        let mut query =
            db.prepare("SELECT id,kind FROM records WHERE event=?1 ORDER BY rowid LIMIT 1024")?;
        value["records"] = json!(
            query
                .query_map([seq], |r| Ok(
                    json!({"record_id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?})
                ))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        );
        value.to_string()
    } else {
        value
    };
    let hash = blake3::hash(value.as_bytes()).to_hex().to_string();
    let tx = db.unchecked_transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO blobs VALUES(?1,?2)",
        params![hash, value.as_bytes()],
    )?;
    tx.execute(
        "INSERT INTO records VALUES(?1,?2,?3,'history_event',?4,?5,NULL,0,?6)",
        params![
            record_id,
            scope,
            format!("event-{record_id}"),
            hash,
            json!({"source":"history_event","event_kind":kind}).to_string(),
            seq
        ],
    )?;
    tx.commit()?;
    Ok(())
}
fn accessible_record_scope(db: &Connection, requester: &Scope, record_id: &str) -> Result<Scope> {
    let (raw, source): (String, Option<String>) = db
        .query_row(
            "SELECT scope,json_extract(metadata,'$.session_id') FROM records WHERE id=?1",
            [record_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .context("record_expired")?;
    let target: Scope = serde_json::from_str(&raw)?;
    ensure!(
        target.owner == requester.owner
            && target.desktop == requester.desktop
            && (requester.session.is_none()
                || requester.session.as_ref() == source.as_ref().or(target.session.as_ref())),
        "record_forbidden"
    );
    Ok(target)
}
fn decode_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryItem> {
    Ok(HistoryItem {
        sequence: row.get(0)?,
        id: row.get(1)?,
        kind: row.get(2)?,
        root_user_message_id: row.get(3)?,
        created_at: row.get(4)?,
        value: serde_json::from_str(&row.get::<_, String>(5)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
    })
}
/// Compare observable terminal state, not sampling timestamps or input fencing counters.
/// Original event payloads retain this metadata; authorization uses the live counters.
pub fn status_identity(status: &Value) -> Value {
    let mut value = status.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("observed_at");
        object.remove("manual_revision");
        if let Some(shell) = object.get_mut("shell").and_then(Value::as_object_mut) {
            shell.remove("reported_at");
        }
        if let Some(sessions) = object.get_mut("sessions").and_then(Value::as_array_mut) {
            for session in sessions {
                *session = status_identity(session);
            }
        }
    }
    value
}

fn insert_status_if_changed(
    db: &Connection,
    scope: &str,
    kind: &str,
    root: Option<&str>,
    status: &Value,
) -> Result<i64> {
    let value = bounded_json(status, 16 * 1024)?;
    let previous: Option<(i64, String)> = db.query_row(
        "SELECT seq,value FROM events WHERE scope=?1 AND kind IN ('pty_status','pty_status_snapshot') ORDER BY seq DESC LIMIT 1",
        [scope], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    if let Some((seq, previous)) = previous {
        let previous: Value = serde_json::from_str(&previous)?;
        if status_identity(&previous) == status_identity(status) {
            return Ok(seq);
        }
    }
    insert_event(db, scope, kind, root, &value)
}

fn bounded_json(value: &Value, max: usize) -> Result<String> {
    let s = serde_json::to_string(value)?;
    ensure!(s.len() <= max, "storage_item_limit");
    Ok(s)
}
fn ensure_scope(db: &Connection, scope: &str) -> Result<()> {
    db.execute("INSERT OR IGNORE INTO scopes(scope) VALUES(?1)", [scope])?;
    Ok(())
}
fn insert_event(
    db: &Connection,
    scope: &str,
    kind: &str,
    root: Option<&str>,
    value: &str,
) -> Result<i64> {
    db.execute(
        "INSERT INTO events(scope,id,kind,root,at,value) VALUES(?1,?2,?3,?4,?5,?6)",
        params![scope, id(), kind, root, now(), value],
    )?;
    Ok(db.last_insert_rowid())
}
fn read_record(db: &Connection, scope: &str, id: &str) -> Result<Record> {
    let (kind,meta,summary,bytes,pending):(String,String,Option<String>,i64,bool)=db.query_row("SELECT r.kind,r.metadata,r.summary,length(b.body),r.pending FROM records r JOIN blobs b ON b.hash=r.hash WHERE r.scope=?1 AND r.id=?2",params![scope,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).context("record_expired")?;
    Ok(Record {
        id: id.into(),
        kind,
        metadata: serde_json::from_str(&meta)?,
        summary: summary.map(|s| serde_json::from_str(&s)).transpose()?,
        bytes: usize::try_from(bytes)?,
        pending,
    })
}
fn record_by_request(db: &Connection, scope: &str, request: &str) -> Result<Option<Record>> {
    let id: Option<String> = db
        .query_row(
            "SELECT id FROM records WHERE scope=?1 AND request=?2",
            params![scope, request],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| read_record(db, scope, &id)).transpose()
}

fn validate_retention(rule: &Retention) -> Result<()> {
    match rule {
        Retention::KeepLast { count } => {
            ensure!(*count > 0 && *count <= i64::MAX as u64, "invalid_keep_last")
        }
        Retention::OlderThan { days } => {
            ensure!(*days > 0 && *days <= 36500, "invalid_retention_days")
        }
        Retention::Before { .. } => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            owner: "owner".into(),
            desktop: "desktop".into(),
            agent: "agent".into(),
            session: Some("session".into()),
        }
    }
    #[test]
    fn uploaded_images_are_scoped_durable_atomic_and_retryable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/images.db");
        let store = Store::open(&path).unwrap();
        let scope = scope();
        let bytes = b"\x89PNG\r\n\x1a\nimage-data";
        let upload = store.image_begin(&scope, "image/png", bytes.len()).unwrap();
        let mut other = scope.clone();
        other.owner = "other".into();
        assert!(store.image_chunk(&other, &upload, 0, bytes).is_err());
        assert!(store.image_chunk(&scope, &upload, 1, bytes).is_err());
        store.image_chunk(&scope, &upload, 0, &bytes[..8]).unwrap();
        assert!(
            store
                .accept_user_images(
                    &scope,
                    "request",
                    "",
                    json!({}),
                    None,
                    true,
                    std::slice::from_ref(&upload),
                    || Ok(())
                )
                .is_err()
        );
        store.image_chunk(&scope, &upload, 8, &bytes[8..]).unwrap();
        store.image_chunk(&scope, &upload, 8, &bytes[8..]).unwrap();
        let accepted = store
            .accept_user_images(
                &scope,
                "request",
                "",
                json!({}),
                None,
                true,
                &[upload],
                || Ok(()),
            )
            .unwrap();
        let history = store.history(&scope, None).unwrap();
        let user = history.items.iter().find(|v| v.kind == "user").unwrap();
        let record_id = user.value["images"][0]["record_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!user.value.to_string().contains("image-data"));
        assert_eq!(store.record_bytes(&scope, &record_id).unwrap().1, bytes);
        assert!(store.record_bytes(&other, &record_id).is_err());
        let retry = store.image_begin(&scope, "image/png", bytes.len()).unwrap();
        store.image_chunk(&scope, &retry, 0, bytes).unwrap();
        let again = store
            .accept_user_images(
                &scope,
                "request",
                "",
                json!({}),
                Some(&accepted.run_id),
                true,
                &[retry],
                || panic!("retry must bypass admission"),
            )
            .unwrap();
        assert!(again.duplicate);
        assert_eq!(again.user_message_id, accepted.user_message_id);
        assert!(
            store
                .image_begin(&scope, "image/png", 4 * 1024 * 1024 + 1)
                .is_err()
        );
        drop(store);
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .record_bytes(&scope, &record_id)
                .unwrap()
                .1,
            bytes
        );
    }
    #[test]
    fn status_deduplicates_across_sources_restarts_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/status.db");
        let scope = scope();
        let state = json!({"session_id":"session","manual_revision":1,"observed_at":1,
            "shell":{"phase":"prompt","cwd":"/tmp","exit_code":0,"reported_at":1},
            "session_process":{"state":"running","pid":42},"desktop_attached":true});
        let store = Store::open(&path).unwrap();
        store.append_status(&scope, state.clone()).unwrap();
        let mut sampled = state.clone();
        sampled["observed_at"] = json!(2);
        sampled["manual_revision"] = json!(2);
        sampled["shell"]["reported_at"] = json!(2);
        store.append_status(&scope, sampled.clone()).unwrap();
        let first = store
            .accept_user(&scope, "one", "hello", sampled.clone())
            .unwrap();
        let second = store
            .accept_user_in_run(
                &scope,
                "two",
                "continue",
                state.clone(),
                Some(&first.run_id),
            )
            .unwrap();
        assert_eq!(first.status_message_id, second.status_message_id);
        assert_eq!(store.history(&scope, None).unwrap().items.len(), 3);
        let task = store
            .delegate(
                &scope,
                &first.root_user_message_id,
                "task",
                "observe",
                sampled.clone(),
                Some(&first.run_id),
            )
            .unwrap();
        assert_eq!(task.status_message_id, first.status_message_id);
        drop(store);
        let store = Store::open(&path).unwrap();
        store.append_status(&scope, sampled.clone()).unwrap();
        assert_eq!(store.history(&scope, None).unwrap().items.len(), 4);
        // A -> B -> A is two meaningful transitions, not a lifetime distinct-value filter.
        sampled["shell"]["phase"] = json!("running");
        store.append_status(&scope, sampled).unwrap();
        store.append_status(&scope, state.clone()).unwrap();
        assert_eq!(store.history(&scope, None).unwrap().items.len(), 6);
        for (index, (field, value)) in [("desktop_attached", json!(false)), ("epoch", json!(2))]
            .into_iter()
            .enumerate()
        {
            let mut changed = state.clone();
            changed[field] = value;
            store.append_status(&scope, changed).unwrap();
            assert_eq!(store.history(&scope, None).unwrap().items.len(), 7 + index);
        }
        let other = Scope {
            agent: "another-agent".into(),
            ..scope.clone()
        };
        store.append_status(&other, state).unwrap();
        assert_eq!(store.history(&other, None).unwrap().items.len(), 1);
    }

    #[test]
    fn snapshots_share_semantic_global_state_but_keep_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("data/status.db")).unwrap();
        let scope = Scope {
            session: None,
            ..scope()
        };
        let state = json!({"scope":"global","observed_at":1,"sessions":[
            {"session_id":"a","observed_at":1,"manual_revision":1,"shell":{"reported_at":1,"phase":"prompt","cwd":"/a","exit_code":0},"foreground_job":{"process_group":3}}
        ]});
        let first = store
            .accept_user(&scope, "a", "one", state.clone())
            .unwrap();
        let mut sampled = state.clone();
        sampled["observed_at"] = json!(2);
        sampled["sessions"][0]["observed_at"] = json!(2);
        sampled["sessions"][0]["manual_revision"] = json!(2);
        sampled["sessions"][0]["shell"]["reported_at"] = json!(2);
        let second = store
            .accept_user_in_run(&scope, "b", "two", sampled.clone(), Some(&first.run_id))
            .unwrap();
        assert_eq!(first.status_message_id, second.status_message_id);
        store.append_status(&scope, sampled.clone()).unwrap();
        assert_eq!(store.history(&scope, None).unwrap().items.len(), 3);
        for field in ["cwd", "exit_code"] {
            let mut changed = sampled.clone();
            changed["sessions"][0]["shell"][field] = json!("changed");
            assert_ne!(status_identity(&changed), status_identity(&sampled));
        }
        sampled["sessions"][0]["foreground_job"]["process_group"] = json!(4);
        let changed = store
            .accept_user_in_run(&scope, "c", "three", sampled, Some(&first.run_id))
            .unwrap();
        assert_ne!(first.status_message_id, changed.status_message_id);
        assert_eq!(store.history(&scope, None).unwrap().items.len(), 5);
    }

    #[test]
    fn user_snapshot_retry_action_fence_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/db");
        let s = Store::open(&path).unwrap();
        let scope = scope();
        let user = s
            .accept_user(&scope, "request", "hello", json!({"state":"unknown"}))
            .unwrap();
        assert!(
            s.accept_user(&scope, "request", "hello", json!({"state":"running"}))
                .unwrap()
                .duplicate
        );
        assert!(
            s.accept_user(&scope, "request", "different", json!({}))
                .is_err()
        );
        let page = s.history(&scope, None).unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].kind, "user");
        assert_eq!(page.items[1].kind, "pty_status_snapshot");
        let action = s
            .prepare_action(&scope, &user.run_id, "action", &json!({"text":"pwd"}))
            .unwrap();
        assert!(!action.duplicate);
        assert!(
            s.prepare_action(&scope, &user.run_id, "action", &json!({"text":"rm"}))
                .is_err()
        );
        drop(s);
        let s = Store::open(&path).unwrap();
        assert!(
            s.prepare_action(&scope, &user.run_id, "action", &json!({"text":"pwd"}))
                .is_err()
        );
    }
    #[test]
    fn bounded_keyset_pages_are_scoped_and_stable_when_appending() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("data/db")).unwrap();
        let scope = scope();
        for i in 0..51 {
            s.append(&scope, "assistant", None, json!({"text":i}))
                .unwrap();
        }
        let a = s.history(&scope, None).unwrap();
        assert_eq!(a.items.len(), 50);
        s.append(&scope, "assistant", None, json!({"text":"new"}))
            .unwrap();
        let b = s.history(&scope, a.cursor.as_deref()).unwrap();
        assert_eq!(b.items.len(), 1);
        assert!(!b.has_more);
        let mut other = scope;
        other.session = Some("other".into());
        assert!(s.history(&other, a.cursor.as_deref()).is_err());
    }
    #[test]
    fn raw_record_survives_invalid_analysis_and_anchors_are_host_owned() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("data/db")).unwrap();
        let scope = scope();
        let user = s.accept_user(&scope, "r", "check", json!({})).unwrap();
        let record = s
            .archive(
                &scope,
                &user.run_id,
                "read-1",
                "text",
                json!({"head":["original"],"tail":["original"]}),
                b"original",
            )
            .unwrap();
        assert!(
            s.analyze(
                &scope,
                &record.id,
                json!({"summary":"bad","key_quotes":[{"record_id":record.id,"text":"invented"}]})
            )
            .is_err()
        );
        assert!(
            s.record(&scope, &record.id, "body", 0)
                .unwrap()
                .record
                .pending
        );
        s.analyze(&scope,&record.id,json!({"summary":"valid","tui_lines":[],"head_anchor":"forged","key_quotes":[{"record_id":record.id,"text":"original"}]})).unwrap();
        let record = s.record(&scope, &record.id, "summary", 0).unwrap().record;
        assert_eq!(record.summary.unwrap()["head_anchor"], json!(["original"]));
    }
    #[test]
    fn analysis_cannot_quote_metadata_as_terminal_body_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("data/db")).unwrap();
        let scope = scope();
        let user = store
            .accept_user(&scope, "request", "read", json!({}))
            .unwrap();
        let record = store
            .archive(
                &scope,
                &user.run_id,
                "read",
                "text",
                json!({"observed_status":{"state":"running"}}),
                b"SOURCE_marker\nsh$",
            )
            .unwrap();
        let invalid = json!({"summary":"source observed","facts":[{"claim":"process running","evidence":{"record_id":record.id,"text":"\"state\":\"running\""},"certainty":"high"}]});
        assert!(
            store
                .analyze(&scope, &record.id, invalid)
                .unwrap_err()
                .to_string()
                .contains("unverified_analysis_evidence")
        );
        let valid = json!({"summary":"source observed","tui_lines":[],"key_quotes":[{"record_id":record.id,"text":"SOURCE_marker"}],"facts":[{"claim":"marker is visible","evidence":{"record_id":record.id,"text":"SOURCE_marker"},"certainty":"observed"}]});
        let digest = store.analyze(&scope, &record.id, valid).unwrap();
        assert_eq!(digest["observed_status"]["state"], "running");
    }
    #[test]
    fn cleanup_respects_pins_invalidates_pages_and_expires_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("data/db")).unwrap();
        let scope = scope();
        for n in 0..51 {
            s.append(&scope, "assistant", None, json!({"text":n}))
                .unwrap();
        }
        let cursor = s.history(&scope, None).unwrap().cursor.unwrap();
        let user = s.accept_user(&scope, "request", "task", json!({})).unwrap();
        let record = s
            .archive(
                &scope,
                &user.run_id,
                "read",
                "text",
                json!({"head":[],"tail":[]}),
                b"evidence",
            )
            .unwrap();
        s.append(
            &scope,
            "assistant",
            Some(&user.root_user_message_id),
            json!({"text":"done"}),
        )
        .unwrap();
        let result = s
            .clean(&scope, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert!(result.pinned >= 3);
        assert!(result.deleted >= 51);
        assert!(s.history(&scope, Some(&cursor)).is_err());
        assert!(s.record(&scope, &record.id, "body", 0).is_ok());
        s.finish_run(&scope, &user.run_id, "completed").unwrap();
        s.clean(&scope, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert!(s.record(&scope, &record.id, "body", 0).is_err());
        assert_eq!(s.history(&scope, None).unwrap().items.len(), 1);
    }
    #[test]
    #[cfg(unix)]
    fn database_open_does_not_change_permissions_of_existing_directories() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let public = dir.path().join("public");
        std::fs::create_dir(&public).unwrap();
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Store::open(&public.join("database")).is_err());
        assert_eq!(
            std::fs::metadata(&public).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!public.join("database").exists());
    }
}

#[cfg(test)]
mod retention_contracts {
    use super::*;
    #[test]
    fn pinned_batch_does_not_hide_later_candidates_and_default_policy_is_inherited() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/store.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("session")).unwrap();
        store
            .owner_retention(
                "owner",
                "desktop",
                Some(Some(Retention::KeepLast { count: 1 })),
            )
            .unwrap();
        assert!(matches!(
            store.retention(&scope, None).unwrap(),
            Some(Retention::KeepLast { count: 1 })
        ));
        let key = scope.key().unwrap();
        {
            let mut db = store.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            for n in 0..600 {
                let seq = insert_event(&tx, &key, "pty_status", None, &json!({"n":n}).to_string())
                    .unwrap();
                if n < 300 {
                    tx.execute("INSERT INTO pins VALUES('active',?1)", [seq])
                        .unwrap();
                }
            }
            tx.commit().unwrap();
        }
        let first = store
            .clean(&scope, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert_eq!(first.candidates, 256);
        assert_eq!(first.deleted, 0);
        let second = store
            .clean(&scope, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert_eq!(second.deleted, 212);
        let third = store
            .clean(&scope, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert_eq!(third.deleted, 87);
        assert_eq!(
            store
                .clean(&scope, &Retention::KeepLast { count: 1 }, true)
                .unwrap()
                .pinned,
            300
        );
        store.retention(&scope, Some(None)).unwrap();
        assert!(store.retention(&scope, None).unwrap().is_none());
    }
    #[test]
    fn request_permission_is_part_of_retry_identity_and_global_can_read_session_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/store.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("s")).unwrap();
        let global = store.agent("owner", "desktop", None).unwrap();
        let user = store
            .accept_user_authorized(&scope, "req", "task", json!({}), None, false)
            .unwrap();
        assert!(
            store
                .accept_user_authorized(&scope, "req", "task", json!({}), None, true)
                .is_err()
        );
        assert!(
            store
                .accept_user_authorized(&scope, "req", "task", json!({}), None, false)
                .unwrap()
                .duplicate
        );
        let record = store
            .archive(&scope, &user.run_id, "read", "text", json!({}), b"original")
            .unwrap();
        assert_eq!(
            store.record_bytes(&global, &record.id).unwrap().1,
            b"original"
        );
    }
}

#[cfg(test)]
mod history_contracts {
    use super::*;
    #[test]
    fn bounded_updates_do_not_drop_earlier_tool_record_references() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/history.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("session")).unwrap();
        let run = store
            .accept_user(&scope, "request", "read", json!({}))
            .unwrap();
        store
            .append_identified(
                &scope,
                "interaction",
                &run.root_user_message_id,
                "unit",
                json!({"text":"工具调用","tools":[{"name":"read_terminal"}]}),
                None,
            )
            .unwrap();
        let mut ids = Vec::new();
        for n in 0..10 {
            for (source, key) in [
                ("tool_call", "call_record_id"),
                ("tool_result", "result_record_id"),
            ] {
                let record = store
                    .archive(
                        &scope,
                        &run.run_id,
                        &format!("{n}/{source}"),
                        "associated_text",
                        json!({"history_unit_id":"unit","source":source}),
                        b"{}",
                    )
                    .unwrap();
                store
                    .unit_update(
                        &scope,
                        "unit",
                        json!({key:record.id,"name":"read_terminal"}),
                    )
                    .unwrap();
                ids.push(record.id);
            }
        }
        let value = store
            .history(&scope, None)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == "unit")
            .unwrap()
            .value;
        assert_eq!(value["updates"].as_array().unwrap().len(), 16);
        let records = value["records"].as_array().unwrap();
        assert_eq!(records.len(), 20);
        assert_eq!(records[0]["record_id"], ids[0]);
        assert_eq!(records[0]["source"], "tool_call");
        assert_eq!(records[1]["source"], "tool_result");
    }
    #[test]
    fn large_interaction_versions_have_distinct_immutable_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/history.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("session")).unwrap();
        store
            .append_identified(
                &scope,
                "interaction",
                "root",
                "unit",
                json!({"text":"说明".repeat(3000),"tools":[{"name":"read_terminal"}]}),
                None,
            )
            .unwrap();
        store
            .unit_update(
                &scope,
                "unit",
                json!({"state":"running","call_record_id":"call"}),
            )
            .unwrap();
        let preview = || {
            store
                .history(&scope, None)
                .unwrap()
                .items
                .into_iter()
                .find(|item| item.id == "unit")
                .unwrap()
                .value
        };
        let first = preview()["record_id"].as_str().unwrap().to_owned();
        assert_ne!(first, "unit");
        assert_eq!(preview()["record_id"], first);
        store
            .unit_update(
                &scope,
                "unit",
                json!({"state":"finished","result_record_id":"result"}),
            )
            .unwrap();
        let second = preview()["record_id"].as_str().unwrap().to_owned();
        assert_ne!(first, second);
        assert_eq!(preview()["record_id"], second);
        let read = |id: &str| {
            let mut cursor = None;
            let mut body = String::new();
            loop {
                let part = store
                    .record_page(&scope, id, "body", cursor.as_deref(), 12288)
                    .unwrap();
                body.push_str(part["body"].as_str().unwrap());
                cursor = part["cursor"].as_str().map(str::to_owned);
                if cursor.is_none() {
                    break;
                }
            }
            serde_json::from_str::<Value>(&body).unwrap()
        };
        assert_eq!(read(&first)["updates"].as_array().unwrap().len(), 1);
        let current = read(&second);
        assert_eq!(current["updates"][1]["state"], "finished");
        assert_eq!(current["updates"][1]["result_record_id"], "result");
        assert_eq!(current["text"], "说明".repeat(3000));
        assert!(current["records"].as_array().unwrap().is_empty());
        store
            .append(
                &scope,
                "assistant",
                Some("next-root"),
                json!({"text":"Next task"}),
            )
            .unwrap();
        store
            .clean(&scope, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert!(
            store
                .record_page(&scope, &first, "body", None, 12288)
                .is_err()
        );
        assert!(
            store
                .record_page(&scope, &second, "body", None, 12288)
                .is_err()
        );
    }
    #[test]
    fn fifty_large_items_remain_bounded_and_originals_are_lossless() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/history.db")).unwrap();
        for count in [0, 49, 50, 51] {
            let scope = store
                .agent("owner", "desktop", Some(&format!("s{count}")))
                .unwrap();
            let text = "中文".repeat(3000);
            for _ in 0..count {
                store
                    .append(&scope, "assistant", None, json!({"text":text}))
                    .unwrap();
            }
            let page = store.history(&scope, None).unwrap();
            assert_eq!(page.items.len(), count.min(50));
            assert_eq!(page.has_more, count > 50);
            assert!(serde_json::to_vec(&page).unwrap().len() < 1024 * 1024);
            if let Some(item) = page.items.first() {
                assert_eq!(item.value["partial"], true);
                let mut cursor = None;
                let mut original = String::new();
                loop {
                    let part = store
                        .record_page(&scope, &item.id, "body", cursor.as_deref(), 12288)
                        .unwrap();
                    original.push_str(part["body"].as_str().unwrap());
                    cursor = part["cursor"].as_str().map(str::to_owned);
                    if cursor.is_none() {
                        break;
                    }
                }
                assert_eq!(
                    serde_json::from_str::<Value>(&original).unwrap()["text"],
                    text
                );
            }
        }
    }
}

#[cfg(test)]
mod tui_anchor_contracts {
    use super::*;
    #[test]
    fn display_and_search_edges_are_distinct_durable_and_validated() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/history.db")).unwrap();
        let scope = store.agent("owner", "desktop", Some("s")).unwrap();
        let user = store
            .accept_user(&scope, "request", "read", json!({}))
            .unwrap();
        let mut lines = (0..40).map(|i| format!("log-{i}")).collect::<Vec<_>>();
        let tui = vec!["┌ input ┐", "status 12%", "└───────┘"];
        lines.extend(tui.iter().map(|s| s.to_string()));
        let body = lines.join("\n");
        let metadata = json!({"head_lines":10,"tail_lines":20,"start":100,"end":143,"view_id":"view-test","alternate_screen":false,"head":{"lines":lines[..10],"positions":(100..110).collect::<Vec<_>>()},"tail":{"lines":lines[23..],"positions":(123..143).collect::<Vec<_>>()}});
        let record = store
            .archive(
                &scope,
                &user.run_id,
                "read",
                "text",
                metadata.clone(),
                body.as_bytes(),
            )
            .unwrap();
        assert_eq!(
            store
                .record_page(&scope, &record.id, "anchors", None, 4096)
                .unwrap()["search_anchor_status"],
            "unclassified"
        );
        assert!(
            store
                .analyze(&scope, &record.id, json!({"summary":"read"}))
                .unwrap_err()
                .to_string()
                .contains("analysis_tui_classification_required")
        );
        assert!(
            store
                .analyze(
                    &scope,
                    &record.id,
                    json!({"summary":"read","tui_lines":["not present"]})
                )
                .is_err()
        );
        let partial = store
            .analyze_observation(
                &scope,
                &record.id,
                json!({"summary":"first page only","tui_lines":[]}),
                false,
            )
            .unwrap();
        assert_eq!(partial["search_anchor_status"], "unclassified_partial_body");
        let digest = store
            .analyze(
                &scope,
                &record.id,
                json!({"summary":"logs and UI","tui_lines":tui}),
            )
            .unwrap();
        assert_eq!(digest["tail_anchor"], metadata["tail"]);
        assert_eq!(digest["head_anchor"], metadata["head"]);
        assert_eq!(digest["search_head_anchor"]["lines"], json!(lines[..10]));
        assert_eq!(digest["search_tail_anchor"]["lines"], json!(lines[20..40]));
        assert_eq!(
            digest["search_tail_anchor"]["positions"],
            json!((120..140).collect::<Vec<_>>())
        );
        let anchors = store
            .record_page(&scope, &record.id, "anchors", None, 4096)
            .unwrap();
        assert_eq!(anchors["search_anchor_status"], "available");
        assert_eq!(anchors["search_tail_anchor"], digest["search_tail_anchor"]);
        assert_eq!(store.view_tui_lines(&scope, "view-test").unwrap().len(), 3);
        assert_eq!(
            store.record_bytes(&scope, &record.id).unwrap().1,
            body.as_bytes()
        );
        let all_tui = store
            .analyze(
                &scope,
                &record.id,
                json!({"summary":"all UI","tui_lines":lines}),
            )
            .unwrap();
        assert_eq!(all_tui["search_anchor_status"], "unavailable_tui_only");
        assert_eq!(all_tui["tail_anchor"], metadata["tail"]);
    }
}

fn validate_image(mime: &str, bytes: &[u8]) -> Result<()> {
    let valid = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    ensure!(valid, "image_format_mismatch");
    Ok(())
}

#[cfg(test)]
mod agent_task_contracts {
    use super::*;

    #[test]
    fn agent_task_preserves_exact_results_across_new_runs_restart_and_retention() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/tasks.db");
        let store = Store::open(&path).unwrap();
        let global = store.agent("owner", "desktop", None).unwrap();
        let child = store.agent("owner", "desktop", Some("session")).unwrap();
        let root = store
            .accept_user(&global, "root", "coordinate", json!({}))
            .unwrap();
        let first = store
            .delegate(
                &child,
                &root.root_user_message_id,
                "one",
                "first",
                json!({}),
                None,
            )
            .unwrap();
        let first_record = id();
        store
            .append_identified(
                &child,
                "assistant",
                &first.root_user_message_id,
                &first_record,
                json!({"text":"第一项任务已完成"}),
                Some(&first.run_id),
            )
            .unwrap();
        store
            .finish_run(&child, &first.run_id, "completed")
            .unwrap();
        let (_, result) = store.agent_task(&global, &first.run_id, 4).unwrap();
        assert_eq!(result["result_text"], "第");
        assert_eq!(result["result_truncated"], true);
        assert_eq!(result["result_record_id"], first_record);
        assert_eq!(result["done"], true);
        let raw = store
            .record_page(&global, &first_record, "body", None, 12288)
            .unwrap();
        assert!(raw.to_string().contains("第一项任务已完成"));

        let second = store
            .delegate(
                &child,
                &root.root_user_message_id,
                "two",
                "second",
                json!({}),
                None,
            )
            .unwrap();
        store
            .append_identified(
                &child,
                "assistant",
                &second.root_user_message_id,
                &id(),
                json!({"text":"second result"}),
                Some(&second.run_id),
            )
            .unwrap();
        store
            .finish_run(&child, &second.run_id, "completed")
            .unwrap();
        let failed = store
            .delegate(
                &child,
                &root.root_user_message_id,
                "failed",
                "fail",
                json!({}),
                None,
            )
            .unwrap();
        store
            .finish_run_with_error(&child, &failed.run_id, "failed", Some("fixture_failure"))
            .unwrap();
        let orphan = store
            .delegate(
                &child,
                &root.root_user_message_id,
                "orphan",
                "pending",
                json!({}),
                None,
            )
            .unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let (_, old) = store.agent_task(&global, &first.run_id, 12288).unwrap();
        assert_eq!(old["result_text"], "第一项任务已完成");
        assert_eq!(old["result_truncated"], false);
        let (_, failed_result) = store.agent_task(&global, &failed.run_id, 12288).unwrap();
        assert_eq!(failed_result["state"], "failed");
        assert_eq!(failed_result["error"], "fixture_failure");
        assert_eq!(failed_result["result_available"], false);
        let (_, orphan_result) = store.agent_task(&global, &orphan.run_id, 12288).unwrap();
        assert_eq!(orphan_result["state"], "orphaned");
        assert_eq!(orphan_result["done"], true);
        assert_eq!(orphan_result["result_available"], false);

        store
            .clean(
                &child,
                &Retention::Before {
                    utc_ms: now() + 60000,
                },
                false,
            )
            .unwrap();
        let (_, expired) = store.agent_task(&global, &first.run_id, 12288).unwrap();
        assert_eq!(expired["state"], "completed");
        assert_eq!(expired["result_available"], false);
        assert_eq!(expired["outcome_available"], false);
        assert!(expired["result_text"].is_null());
    }

    #[test]
    fn agent_task_rejects_other_owners_desktops_sessions_and_non_delegated_runs() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/tasks.db")).unwrap();
        let global = store.agent("owner", "desktop", None).unwrap();
        let child = store.agent("owner", "desktop", Some("session")).unwrap();
        let root = store
            .accept_user(&global, "root", "coordinate", json!({}))
            .unwrap();
        let task = store
            .delegate(
                &child,
                &root.root_user_message_id,
                "task",
                "work",
                json!({}),
                None,
            )
            .unwrap();
        for caller in [
            store.agent("other", "desktop", None).unwrap(),
            store.agent("owner", "other", None).unwrap(),
        ] {
            assert_eq!(
                store
                    .agent_task(&caller, &task.run_id, 1024)
                    .unwrap_err()
                    .to_string(),
                "agent_task_not_found"
            );
        }
        assert_eq!(
            store
                .agent_task(&child, &task.run_id, 1024)
                .unwrap_err()
                .to_string(),
            "global_agent_required"
        );
        let independent = store
            .accept_user(&child, "user", "own work", json!({}))
            .unwrap();
        for run in [&root.run_id, &independent.run_id, &id()] {
            assert_eq!(
                store
                    .agent_task(&global, run, 1024)
                    .unwrap_err()
                    .to_string(),
                "agent_task_not_found"
            );
        }
    }
}

#[cfg(test)]
mod global_conversation_contracts {
    use super::*;

    #[test]
    fn globals_are_idempotent_isolated_and_preserve_legacy_history_after_restart() {
        let path = std::env::temp_dir()
            .join(format!("global-{}", id()))
            .join("history.sqlite3");
        let legacy_id;
        let new_id;
        {
            let store = Store::open(&path).unwrap();
            let legacy = store.agent("owner", "desktop", None).unwrap();
            legacy_id = legacy.agent.clone();
            store
                .accept_user(&legacy, "old-request", "旧全局历史", json!({}))
                .unwrap();
            store
                .append(&legacy, "assistant", None, json!({"text":"旧回复"}))
                .unwrap();
            let one = store
                .create_global("owner", "desktop", "new-request")
                .unwrap();
            new_id = one.agent.clone();
            assert_eq!(
                one,
                store
                    .create_global("owner", "desktop", "new-request")
                    .unwrap()
            );
            let two = store
                .create_global("owner", "desktop", "another-request")
                .unwrap();
            assert_ne!(one.agent, two.agent);
            assert!(one.session.is_none());
            assert!(
                store
                    .agent_by_id("other", "desktop", &one.agent)
                    .unwrap()
                    .is_none()
            );
            assert!(
                store
                    .agent_by_id("owner", "other", &one.agent)
                    .unwrap()
                    .is_none()
            );
            store
                .accept_user(&one, "same-request", "新对话一", json!({}))
                .unwrap();
            store
                .accept_user(&two, "same-request", "新对话二", json!({}))
                .unwrap();
            let reply = store
                .append(&one, "assistant", None, json!({"text":"独立回复"}))
                .unwrap();
            let terminal = store.agent("owner", "desktop", Some("terminal")).unwrap();
            store
                .accept_user(&terminal, "same-request", "终端历史", json!({}))
                .unwrap();
            let page = store.global_page("owner", "desktop", None).unwrap();
            let rows = page["conversations"].as_array().unwrap();
            assert_eq!(rows.len(), 3);
            let summary = rows
                .iter()
                .find(|r| r["scope"]["agent"] == one.agent)
                .unwrap();
            assert_eq!(summary["title"], "新对话一");
            assert_eq!(summary["preview"], "独立回复");
            assert_eq!(summary["last_reply_sequence"], reply);
            assert_eq!(
                store
                    .history(&two, None)
                    .unwrap()
                    .items
                    .iter()
                    .filter(|i| i.kind == "assistant")
                    .count(),
                0
            );
            assert_eq!(rows.iter().filter(|r| r["legacy"] == true).count(), 1);
        }
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(
                store.agent("owner", "desktop", None).unwrap().agent,
                legacy_id
            );
            assert_eq!(
                store
                    .create_global("owner", "desktop", "new-request")
                    .unwrap()
                    .agent,
                new_id
            );
            let page = store.global_page("owner", "desktop", None).unwrap();
            assert_eq!(page["conversations"].as_array().unwrap().len(), 3);
            let legacy = store
                .agent_by_id("owner", "desktop", &legacy_id)
                .unwrap()
                .unwrap();
            assert!(
                store
                    .history(&legacy, None)
                    .unwrap()
                    .items
                    .iter()
                    .any(|i| i.value["text"] == "旧回复")
            );
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn global_catalog_pages_do_not_drop_older_conversations_or_include_other_scopes() {
        let path = std::env::temp_dir()
            .join(format!("global-page-{}", id()))
            .join("history.sqlite3");
        let store = Store::open(&path).unwrap();
        assert!(
            store.global_page("owner", "desktop", None).unwrap()["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for i in 0..55 {
            store
                .create_global("owner", "desktop", &format!("request-{i}"))
                .unwrap();
        }
        store.create_global("other", "desktop", "request").unwrap();
        store.agent("owner", "desktop", Some("terminal")).unwrap();
        let mut cursor = None;
        let mut ids = std::collections::HashSet::new();
        loop {
            let page = store.global_page("owner", "desktop", cursor).unwrap();
            assert!(page.to_string().len() < 65536);
            for row in page["conversations"].as_array().unwrap() {
                assert!(ids.insert(row["scope"]["agent"].as_str().unwrap().to_owned()));
            }
            cursor = page["cursor"].as_i64();
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids.len(), 55);
        drop(store);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
