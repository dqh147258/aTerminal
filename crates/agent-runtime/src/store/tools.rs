//! Bounded, scope-bound discovery. Cursor signatures reuse the Store's private key.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    session_id: Option<String>,
    kind: Option<String>,
    after_ms: Option<i64>,
    before_ms: Option<i64>,
    cursor: Option<String>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tasks {
    root_user_message_id: Option<String>,
    state: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}
#[derive(Serialize, Deserialize)]
struct SearchCursor {
    scope: String,
    filter: String,
    generation: i64,
    high: i64,
    before: i64,
    // -1 scans event JSON; nonnegative values scan record rowids within this event.
    source: i64,
    offset: usize,
}
type SearchEvent = (i64, String, String, i64, String);
type SearchText = (Option<String>, String, i64, usize, Vec<u8>);
pub(super) fn interrupt_native_commands(db: &Connection) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='tool_commands')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        db.execute("UPDATE tool_commands SET value=json_set(value,'$.state','unknown','$.reason','desktop_restart','$.final',json('true'),'$.exit_code',NULL) WHERE json_extract(value,'$.source')='native_program' AND COALESCE(json_extract(value,'$.final'),0)=0",[])?;
    }
    Ok(())
}
impl Store {
    /// Bounded traversal of full retained event/record text. An empty page with a cursor
    /// means scanning is incomplete, rather than a claim that the query has no matches.
    pub fn search_history(&self, caller: &Scope, args: Value) -> Result<Value> {
        let args: Search =
            serde_json::from_value(args).context("invalid_search_history_arguments")?;
        ensure!(
            !args.query.trim().is_empty() && args.query.len() <= 512,
            "invalid_history_query"
        );
        ensure!(
            args.kind
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 64),
            "invalid_history_kind"
        );
        ensure!(
            args.after_ms
                .zip(args.before_ms)
                .is_none_or(|(a, b)| a <= b),
            "invalid_history_time_range"
        );
        if let Some(session) = &caller.session {
            ensure!(
                args.session_id.as_ref().is_none_or(|s| s == session),
                "cross_session_tool_rejected"
            );
        }
        let session = caller.session.as_ref().or(args.session_id.as_ref());
        let limit = args.limit.unwrap_or(20);
        ensure!((1..=50).contains(&limit), "invalid_history_limit");
        let filter = blake3::hash(
            serde_json::to_string(&json!([
                args.query,
                session,
                args.kind,
                args.after_ms,
                args.before_ms,
                limit
            ]))?
            .as_bytes(),
        )
        .to_hex()
        .to_string();
        let db = self.db.lock().unwrap();
        let generation:i64=db.query_row("SELECT COALESCE(SUM(generation),0) FROM scopes WHERE json_extract(scope,'$.owner')=?1 AND json_extract(scope,'$.desktop')=?2 AND (?3 IS NULL OR json_extract(scope,'$.session')=?3)",params![caller.owner,caller.desktop,session],|r|r.get(0))?;
        let high: i64 =
            db.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
        let mut cursor = if let Some(token) = &args.cursor {
            let c: SearchCursor = self.decode(token)?;
            ensure!(
                c.scope == caller.key()? && c.filter == filter,
                "cursor_scope_mismatch"
            );
            ensure!(c.generation == generation, "cursor_expired");
            c
        } else {
            SearchCursor {
                scope: caller.key()?,
                filter,
                generation,
                high,
                before: high,
                source: -1,
                offset: 0,
            }
        };
        let needle = args.query.to_lowercase();
        let mut items = Vec::new();
        let mut scanned_bytes = 0usize;
        let mut scanned = 0usize;
        // Cap database reads and CPU independently from the number of successful matches.
        while scanned < 128 && scanned_bytes < 256 * 1024 && items.len() < limit {
            let event:Option<SearchEvent>=db.query_row("SELECT seq,id,kind,at,scope FROM events WHERE seq<=?1 AND seq<=?2 AND json_extract(scope,'$.owner')=?3 AND json_extract(scope,'$.desktop')=?4 AND (?5 IS NULL OR json_extract(scope,'$.session')=?5) AND (?6 IS NULL OR at>=?6) AND (?7 IS NULL OR at<=?7) ORDER BY seq DESC LIMIT 1",params![cursor.before,cursor.high,caller.owner,caller.desktop,session,args.after_ms,args.before_ms],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let Some((seq, event_id, event_kind, at, scope)) = event else {
                break;
            };
            if cursor.before != seq {
                cursor.before = seq;
                cursor.source = -1;
                cursor.offset = 0;
            }
            scanned += 1;
            let available = (256 * 1024 - scanned_bytes).min(65536);
            let candidate: Option<SearchText> = if cursor.source < 0 {
                if args.kind.as_ref().is_none_or(|kind| kind == &event_kind) {
                    Some(db.query_row("SELECT LENGTH(CAST(value AS BLOB)),SUBSTR(CAST(value AS BLOB),?2,?3) FROM events WHERE seq=?1",params![seq,cursor.offset as i64+1,available as i64],|r|Ok((None,event_kind.clone(),-1,r.get::<_,i64>(0)? as usize,r.get(1)?)))?)
                } else {
                    None
                }
            } else {
                db.query_row("SELECT records.id,records.kind,records.rowid,LENGTH(blobs.body),SUBSTR(blobs.body,?3,?4) FROM records JOIN blobs ON blobs.hash=records.hash WHERE event=?1 AND records.rowid>=?2 AND COALESCE(json_extract(records.metadata,'$.binary'),0)=0 AND (?5 IS NULL OR records.kind=?5) ORDER BY records.rowid LIMIT 1",params![seq,cursor.source,cursor.offset as i64+1,available as i64,args.kind],|r|Ok((Some(r.get(0)?),r.get(1)?,r.get(2)?,r.get::<_,i64>(3)? as usize,r.get(4)?))).optional()?
            };
            let Some((record_id, kind, rowid, length, chunk)) = candidate else {
                if cursor.source < 0 {
                    cursor.source = 0;
                    cursor.offset = 0;
                } else {
                    cursor.before = seq - 1;
                    cursor.source = -1;
                    cursor.offset = 0;
                }
                continue;
            };
            scanned_bytes += chunk.len();
            let text = String::from_utf8_lossy(&chunk);
            if let Some(snippet) = snippet(&text, &needle) {
                let target: Scope = serde_json::from_str(&scope)?;
                items.push(json!({"event_id":event_id,"record_id":record_id,"kind":kind,"created_at":at,"session_id":target.session,"agent_id":target.agent,"snippet":snippet}));
                cursor.before = seq - 1;
                cursor.source = -1;
                cursor.offset = 0;
            } else if cursor.offset.saturating_add(chunk.len()) >= length {
                cursor.source = if rowid < 0 { 0 } else { rowid + 1 };
                cursor.offset = 0;
            } else {
                // Include enough UTF-8/case-fold overlap to find queries across chunk edges.
                let overlap = (args.query.len() * 4 + 8).min(4096);
                if chunk.len() <= overlap {
                    break;
                }
                cursor.source = rowid;
                cursor.offset += chunk.len() - overlap;
            }
        }
        let more:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE seq<=?1 AND seq<=?2 AND json_extract(scope,'$.owner')=?3 AND json_extract(scope,'$.desktop')=?4 AND (?5 IS NULL OR json_extract(scope,'$.session')=?5) AND (?6 IS NULL OR at>=?6) AND (?7 IS NULL OR at<=?7))",params![cursor.before,cursor.high,caller.owner,caller.desktop,session,args.after_ms,args.before_ms],|r|r.get(0))?;
        let token = if more {
            let bytes = serde_json::to_vec(&cursor)?;
            Some(format!(
                "{}.{}",
                URL_SAFE_NO_PAD.encode(&bytes),
                blake3::keyed_hash(&self.cursor_key, &bytes).to_hex()
            ))
        } else {
            None
        };
        Ok(
            json!({"items":items,"cursor":token,"has_more":more,"generation":generation,"snapshot_watermark":cursor.high,"scanned":scanned,"scanned_bytes":scanned_bytes,"scan_incomplete":more}),
        )
    }
    pub fn list_agent_tasks(&self, caller: &Scope, args: Value) -> Result<Value> {
        ensure!(caller.session.is_none(), "global_agent_required");
        let args: Tasks = serde_json::from_value(args).context("invalid_agent_tasks_arguments")?;
        let limit = args.limit.unwrap_or(20);
        ensure!((1..=50).contains(&limit), "invalid_task_limit");
        let kind = format!(
            "tasks/{}",
            serde_json::to_string(&json!([args.root_user_message_id, args.state, limit]))?
        );
        let db = self.db.lock().unwrap();
        let high: i64 =
            db.query_row("SELECT COALESCE(MAX(rowid),0) FROM runs", [], |r| r.get(0))?;
        let mut cursor = if let Some(token) = args.cursor {
            let c: Cursor = self.decode(&token)?;
            ensure!(
                c.scope == caller.key()? && c.kind == kind,
                "cursor_scope_mismatch"
            );
            c
        } else {
            Cursor {
                scope: caller.key()?,
                kind,
                generation: 0,
                before: high + 1,
                high,
            }
        };
        let mut statement = db.prepare("SELECT rowid,id,scope,root,state FROM runs WHERE rowid<?1 AND rowid<=?2 AND json_extract(scope,'$.owner')=?3 AND json_extract(scope,'$.desktop')=?4 AND json_extract(scope,'$.session') IS NOT NULL AND EXISTS(SELECT 1 FROM delegations WHERE run=runs.id AND scope=runs.scope) AND (?5 IS NULL OR root=?5) AND (?6 IS NULL OR state=?6) ORDER BY rowid DESC LIMIT ?7")?;
        let mut rows = statement
            .query_map(
                params![
                    cursor.before,
                    cursor.high,
                    caller.owner,
                    caller.desktop,
                    args.root_user_message_id,
                    args.state,
                    (limit + 1) as i64
                ],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let mut items = Vec::new();
        for (seq, id, scope, root, state) in rows {
            cursor.before = seq;
            let scope: Scope = serde_json::from_str(&scope)?;
            items.push(json!({"task_id":id,"agent_id":scope.agent,"session_id":scope.session,"root_user_message_id":root,"state":state}));
        }
        Ok(
            json!({"tasks":items,"cursor":if more{Some(self.encode(&cursor)?)}else{None},"has_more":more}),
        )
    }
}
fn snippet(text: &str, needle: &str) -> Option<String> {
    // Work in chars: Unicode case expansion must not turn an offset into an invalid UTF-8 slice.
    let chars: Vec<char> = text.chars().take(65536).collect();
    let mut lowered = String::new();
    let mut origins = Vec::new();
    for (index, c) in chars.iter().enumerate() {
        for lower in c.to_lowercase() {
            lowered.push(lower);
            origins.push(index);
        }
    }
    let byte = lowered.find(needle)?;
    let position = *origins.get(lowered[..byte].chars().count())?;
    let start = position.saturating_sub(80);
    Some(chars.iter().skip(start).take(320).collect())
}

fn command_schema(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS tool_commands(id TEXT PRIMARY KEY,scope TEXT NOT NULL,session TEXT NOT NULL,action TEXT NOT NULL,event INTEGER NOT NULL REFERENCES events(seq) ON DELETE CASCADE,value TEXT NOT NULL,UNIQUE(scope,action)); CREATE INDEX IF NOT EXISTS tool_commands_session ON tool_commands(session);")?;
    Ok(())
}
impl Store {
    /// Called before PTY submission. Command rows follow ordinary event retention.
    pub fn begin_command(
        &self,
        caller: &Scope,
        run: &str,
        action: &str,
        session: &str,
        mut value: Value,
    ) -> Result<String> {
        ensure!(
            caller.session.as_deref().is_none_or(|id| id == session),
            "cross_session_tool_rejected"
        );
        let mut db = self.db.lock().unwrap();
        command_schema(&db)?;
        let tx = db.transaction()?;
        let key = caller.key()?;
        ensure!(
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND scope=?2 AND state='running')",
                params![run, key],
                |r| r.get::<_, bool>(0)
            )?,
            "run_not_active"
        );
        ensure!(
            !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM tool_commands WHERE scope=?1 AND action=?2)",
                params![key, action],
                |r| r.get::<_, bool>(0)
            )?,
            "command_already_submitted"
        );
        let command = id();
        value["command_id"] = json!(command);
        value["run_id"] = json!(run);
        value["session_id"] = json!(session);
        value["evidence_source"] = json!("host_submission_and_session_shell_hook");
        value["application_task"] =
            json!({"state":"unknown","evidence_source":"no_application_adapter"});
        let event = insert_event(
            &tx,
            &key,
            "command_submission",
            None,
            &json!({"command_id":command,"session_id":session}).to_string(),
        )?;
        let event_id: String =
            tx.query_row("SELECT id FROM events WHERE seq=?1", [event], |r| r.get(0))?;
        value["evidence_event_id"] = json!(event_id);
        tx.execute(
            "INSERT INTO tool_commands VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                command,
                key,
                session,
                action,
                event,
                bounded_json(&value, 65536)?
            ],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO pins(run,event) VALUES(?1,?2)",
            params![run, event],
        )?;
        tx.commit()?;
        Ok(command)
    }
    pub fn command(&self, caller: &Scope, command: &str) -> Result<Value> {
        let db = self.db.lock().unwrap();
        command_schema(&db)?;
        let value:String=db.query_row("SELECT value FROM tool_commands WHERE id=?1 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3 AND (?4 IS NULL OR session=?4)",params![command,caller.owner,caller.desktop,caller.session],|r|r.get(0)).optional()?.context("command_not_found_or_expired")?;
        Ok(serde_json::from_str(&value)?)
    }
    /// Submission acceptance is independent from (possibly already invalidated) completion.
    pub fn set_command_acceptance(
        &self,
        caller: &Scope,
        command: &str,
        accepted: bool,
    ) -> Result<()> {
        let db = self.db.lock().unwrap();
        command_schema(&db)?;
        db.execute("UPDATE tool_commands SET value=json_set(value,'$.accepted',json(?5)) WHERE id=?1 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3 AND (?4 IS NULL OR session=?4)",params![command,caller.owner,caller.desktop,caller.session,if accepted{"true"}else{"false"}])?;
        Ok(())
    }
    /// A final result is immutable. Concurrent observers cannot overwrite completed/unknown evidence.
    pub fn update_command(&self, caller: &Scope, command: &str, value: &Value) -> Result<()> {
        let db = self.db.lock().unwrap();
        command_schema(&db)?;
        db.execute("UPDATE tool_commands SET value=json_set(?5,'$.run_id',json_extract(value,'$.run_id')) WHERE id=?1 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3 AND (?4 IS NULL OR session=?4) AND COALESCE(json_extract(value,'$.final'),0)=0",params![command,caller.owner,caller.desktop,caller.session,bounded_json(value,65536)?])?;
        Ok(())
    }
    /// Every PTY write must call this, including raw text, keys and other Run/Agent writers.
    pub fn invalidate_command_writes(
        &self,
        caller: &Scope,
        session: &str,
        action: &str,
    ) -> Result<()> {
        let db = self.db.lock().unwrap();
        command_schema(&db)?;
        db.execute("UPDATE tool_commands SET value=json_set(value,'$.state','unknown','$.reason','intervening_agent_input','$.final',json('true'),'$.exit_code',NULL) WHERE session=?1 AND json_extract(scope,'$.owner')=?2 AND json_extract(scope,'$.desktop')=?3 AND NOT(scope=?4 AND action=?5) AND COALESCE(json_extract(value,'$.final'),0)=0 AND COALESCE(json_extract(value,'$.source'),'')!='native_program'",params![session,caller.owner,caller.desktop,caller.key()?,action])?;
        Ok(())
    }
}

#[cfg(test)]
mod toolset_tests {
    use super::*;
    #[test]
    fn native_live_rows_ignore_pty_writes_and_restart_marks_them_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private/commands.db");
        let store = Store::open(&path).unwrap();
        let scope = store.agent("owner", "desktop", Some("session")).unwrap();
        let root = store.accept_user(&scope, "r", "work", json!({})).unwrap();
        let native=store.begin_command(&scope,&root.run_id,"native","session",json!({"source":"native_program","state":"running","accepted":true,"final":false,"exit_code":null})).unwrap();
        let pty = store
            .begin_command(
                &scope,
                &root.run_id,
                "pty",
                "session",
                json!({"state":"submitted","final":false}),
            )
            .unwrap();
        assert_eq!(
            store.command(&scope, &native).unwrap()["run_id"],
            root.run_id
        );
        store
            .invalidate_command_writes(&scope, "session", "new-pty-write")
            .unwrap();
        assert_eq!(store.command(&scope, &native).unwrap()["state"], "running");
        assert_eq!(store.command(&scope, &native).unwrap()["final"], false);
        assert_eq!(store.command(&scope, &pty).unwrap()["state"], "unknown");
        drop(store);
        let restored = Store::open(&path).unwrap();
        let row = restored.command(&scope, &native).unwrap();
        assert_eq!(row["state"], "unknown");
        assert_eq!(row["final"], true);
        assert_eq!(row["reason"], "desktop_restart");
        assert!(row["exit_code"].is_null());
    }
    #[test]
    fn search_cursors_bind_filters_scope_watermark_and_retention() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/db")).unwrap();
        let global = store.agent("owner", "desktop", None).unwrap();
        let child = store.agent("owner", "desktop", Some("one")).unwrap();
        let foreign = store.agent("foreign", "desktop", Some("one")).unwrap();
        for index in 0..3 {
            store
                .append(
                    &child,
                    "assistant",
                    None,
                    json!({"text":format!("needle {index}")}),
                )
                .unwrap();
        }
        store
            .append(
                &foreign,
                "assistant",
                None,
                json!({"text":"needle foreign"}),
            )
            .unwrap();
        let query = json!({"query":"needle","kind":"assistant","limit":1,"session_id":"one"});
        let first = store.search_history(&global, query.clone()).unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 1);
        assert!(!first.to_string().contains("foreign"));
        let cursor = first["cursor"].as_str().unwrap();
        store
            .append(&child, "assistant", None, json!({"text":"needle new"}))
            .unwrap();
        let mut next = query.clone();
        next["cursor"] = json!(cursor);
        let second = store.search_history(&global, next.clone()).unwrap();
        assert_ne!(
            first["items"][0]["event_id"],
            second["items"][0]["event_id"]
        );
        assert!(!second.to_string().contains("needle new"));
        let mut changed = next.clone();
        changed["query"] = json!("different");
        assert!(store.search_history(&global, changed).is_err());
        assert!(store.search_history(&child, next).is_err());
        assert!(
            store
                .search_history(&child, json!({"query":"needle","session_id":"two"}))
                .is_err()
        );
        store
            .clean(&child, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        let mut expired = query;
        expired["cursor"] = json!(cursor);
        assert_eq!(
            store
                .search_history(&global, expired)
                .unwrap_err()
                .to_string(),
            "cursor_expired"
        );
    }
    #[test]
    fn search_reports_exact_record_and_unicode_snippet() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/db")).unwrap();
        let scope = store.agent("o", "d", Some("s")).unwrap();
        let run = store
            .accept_user(&scope, "run", "inspect", json!({}))
            .unwrap();
        let record = store
            .archive(
                &scope,
                &run.run_id,
                "evidence",
                "text",
                json!({}),
                "İİİ 结果 needle end".as_bytes(),
            )
            .unwrap();
        let result = store
            .search_history(&scope, json!({"query":"结果","kind":"text"}))
            .unwrap();
        assert_eq!(result["items"][0]["record_id"], record.id);
        assert!(
            result["items"][0]["snippet"]
                .as_str()
                .unwrap()
                .contains("结果")
        );
        assert!(snippet("İİİ marker", "marker").unwrap().contains("marker"));
    }
    #[test]
    fn command_results_survive_restart_and_new_runs_with_scope_and_retention() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/db");
        let store = Store::open(&path).unwrap();
        let global = store.agent("o", "d", None).unwrap();
        let session = store.agent("o", "d", Some("s")).unwrap();
        let other = store.agent("o", "d", Some("other")).unwrap();
        let foreign = store.agent("x", "d", None).unwrap();
        let run = store
            .accept_user(&global, "run", "command", json!({}))
            .unwrap();
        let command = store
            .begin_command(
                &global,
                &run.run_id,
                "action",
                "s",
                json!({"command":"false","state":"submitted","final":false}),
            )
            .unwrap();
        assert!(
            store
                .begin_command(&global, &run.run_id, "action", "s", json!({}))
                .is_err()
        );
        let mut value = store.command(&session, &command).unwrap();
        value["state"] = json!("completed");
        value["exit_code"] = json!(1);
        value["final"] = json!(true);
        store.update_command(&global, &command, &value).unwrap();
        store
            .set_command_acceptance(&global, &command, true)
            .unwrap();
        assert!(store.command(&other, &command).is_err());
        assert!(store.command(&foreign, &command).is_err());
        store.finish_run(&global, &run.run_id, "completed").unwrap();
        store.accept_user(&global, "new", "new", json!({})).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let retained = store.command(&session, &command).unwrap();
        assert_eq!(retained["exit_code"], 1);
        assert_eq!(retained["accepted"], true);
        assert!(retained["evidence_event_id"].is_string());
        store
            .clean(&global, &Retention::KeepLast { count: 1 }, false)
            .unwrap();
        assert!(store.command(&global, &command).is_err());
    }
    #[test]
    fn raw_writes_invalidate_only_pending_associations_and_acceptance_is_independent() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/db")).unwrap();
        let global = store.agent("o", "d", None).unwrap();
        let run = store
            .accept_user(&global, "r", "command", json!({}))
            .unwrap();
        let command = store
            .begin_command(
                &global,
                &run.run_id,
                "a",
                "s",
                json!({"state":"submitted","final":false}),
            )
            .unwrap();
        store.invalidate_command_writes(&global, "s", "a").unwrap();
        assert_eq!(
            store.command(&global, &command).unwrap()["state"],
            "submitted"
        );
        store
            .invalidate_command_writes(&global, "s", "raw")
            .unwrap();
        store
            .set_command_acceptance(&global, &command, true)
            .unwrap();
        let result = store.command(&global, &command).unwrap();
        assert_eq!(result["state"], "unknown");
        assert_eq!(result["accepted"], true);
        store
            .update_command(
                &global,
                &command,
                &json!({"state":"completed","exit_code":0}),
            )
            .unwrap();
        assert_eq!(
            store.command(&global, &command).unwrap()["state"],
            "unknown"
        );
    }
}

#[cfg(test)]
mod toolset_chunk_tests {
    use super::*;
    #[test]
    fn bounded_search_continues_through_large_record_and_chunk_edges() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("data/db")).unwrap();
        let scope = store.agent("o", "d", Some("s")).unwrap();
        let run = store.accept_user(&scope, "r", "search", json!({})).unwrap();
        let mut text = "x".repeat(400 * 1024);
        text.replace_range(65532..65544, "chunk_needle");
        text.push_str(" late_needle");
        let record = store
            .archive(
                &scope,
                &run.run_id,
                "large",
                "text",
                json!({}),
                text.as_bytes(),
            )
            .unwrap();
        let edge = store
            .search_history(&scope, json!({"query":"chunk_needle","kind":"text"}))
            .unwrap();
        assert_eq!(edge["items"][0]["record_id"], record.id);
        let mut query = json!({"query":"late_needle","kind":"text"});
        let first = store.search_history(&scope, query.clone()).unwrap();
        assert!(first["items"].as_array().unwrap().is_empty());
        assert_eq!(first["scan_incomplete"], true);
        assert!(first["scanned_bytes"].as_u64().unwrap() <= 256 * 1024);
        query["cursor"] = first["cursor"].clone();
        let next = store.search_history(&scope, query).unwrap();
        assert_eq!(next["items"][0]["record_id"], record.id);
    }
}
