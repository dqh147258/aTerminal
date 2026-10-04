//! User-only authorization persistence. Model tool dispatch must not expose mutations here.
use super::*;

pub(super) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS agent_permissions(scope TEXT PRIMARY KEY, mode TEXT NOT NULL, full INTEGER NOT NULL, revision INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS human_pending(id TEXT PRIMARY KEY, scope TEXT NOT NULL, authority TEXT NOT NULL, run TEXT NOT NULL, action TEXT NOT NULL, fingerprint TEXT, value TEXT NOT NULL, state TEXT NOT NULL, response TEXT, expires INTEGER NOT NULL, UNIQUE(scope,action));
      CREATE INDEX IF NOT EXISTS human_pending_scope ON human_pending(scope,state);
      CREATE INDEX IF NOT EXISTS human_pending_authority ON human_pending(authority,state);
      CREATE INDEX IF NOT EXISTS human_pending_expiration ON human_pending(state,expires);
      CREATE TABLE IF NOT EXISTS authorization_rules(id TEXT PRIMARY KEY, owner TEXT NOT NULL, desktop TEXT NOT NULL, fingerprint TEXT NOT NULL, value TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0, UNIQUE(owner,desktop,fingerprint));
      CREATE TABLE IF NOT EXISTS human_responses(scope TEXT NOT NULL, request TEXT NOT NULL, hash TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(scope,request));
      CREATE TABLE IF NOT EXISTS approval_details(pending TEXT PRIMARY KEY REFERENCES human_pending(id), value TEXT NOT NULL);
      UPDATE human_pending SET state='interrupted',value=json_set(value,'$.terminal_at',CAST(strftime('%s','now') AS INTEGER)*1000) WHERE state IN ('pending','resolved');")?;
    Ok(())
}
const TERMINAL_PENDING_LIMIT: i64 = 64;
const TERMINAL_PENDING_TTL: i64 = 24 * 60 * 60 * 1000;

/// Retain actionable requests and a bounded recent set of failure/status cards.
/// Response receipts remain intact, so retries of an acknowledged user RPC are
/// still idempotent even after its retired status card/details have been removed.
fn retire_pending(db: &Connection, key: &str, at: i64) -> Result<()> {
    db.execute("UPDATE human_pending SET state='expired',value=json_set(value,'$.terminal_at',expires) WHERE state IN ('pending','resolved') AND expires<=?1", [at])?;
    let terminal = "state IN ('cancelled','expired','interrupted','superseded')";
    let visible = "(scope=?1 OR authority=?1)";
    let ended =
        "COALESCE(json_extract(value,'$.terminal_at'),json_extract(value,'$.created_at'),expires)";
    let retired = format!(
        "SELECT id FROM human_pending WHERE {visible} AND {terminal} AND ({ended}<=?2 OR id NOT IN (SELECT id FROM human_pending WHERE {visible} AND {terminal} AND {ended}>?2 ORDER BY {ended} DESC,id DESC LIMIT ?3))"
    );
    let params = params![
        key,
        at.saturating_sub(TERMINAL_PENDING_TTL),
        TERMINAL_PENDING_LIMIT
    ];
    db.execute(
        &format!("DELETE FROM approval_details WHERE pending IN ({retired})"),
        params,
    )?;
    db.execute(
        &format!("DELETE FROM human_pending WHERE id IN ({retired})"),
        params,
    )?;
    Ok(())
}
fn permissions(db: &Connection, scope: &str) -> Result<Value> {
    Ok(db.query_row("SELECT mode,full,revision FROM agent_permissions WHERE scope=?1",[scope],|r| Ok(json!({"permission_mode":r.get::<_,String>(0)?,"full_authorization":r.get::<_,bool>(1)?,"revision":r.get::<_,i64>(2)?,"can_mutate":true}))).optional()?.unwrap_or_else(||json!({"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true})))
}
fn pending_value(db: &Connection, id: &str) -> Result<Value> {
    let (value, state, response): (String, String, Option<String>) = db.query_row(
        "SELECT value,state,response FROM human_pending WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut value: Value = serde_json::from_str(&value)?;
    value["state"] = json!(state);
    if let Some(response) = response {
        value["response"] = serde_json::from_str(&response)?;
    }
    if matches!(state.as_str(), "interrupted" | "expired" | "cancelled") {
        value["reason"] = json!(format!("request_{state}"));
    }
    Ok(value)
}
impl Store {
    pub fn permissions(&self, scope: &Scope) -> Result<Value> {
        permissions(&self.db.lock().unwrap(), &scope.key()?)
    }
    pub fn set_permissions(
        &self,
        scope: &Scope,
        expected: u64,
        mode: Option<&str>,
        full: Option<bool>,
    ) -> Result<Value> {
        ensure!(
            mode.is_some() || full.is_some(),
            "permission_change_required"
        );
        ensure!(
            mode.is_none_or(|m| matches!(m, "ask" | "read_only")),
            "invalid_permission_mode"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let key = scope.key()?;
        let old = permissions(&tx, &key)?;
        ensure!(
            old["revision"].as_u64() == Some(expected),
            "permission_revision_conflict"
        );
        let mode = mode.unwrap_or(old["permission_mode"].as_str().unwrap());
        let full = full.unwrap_or(old["full_authorization"] == true) && mode != "read_only";
        tx.execute("INSERT INTO agent_permissions VALUES(?1,?2,?3,?4) ON CONFLICT(scope) DO UPDATE SET mode=excluded.mode,full=excluded.full,revision=excluded.revision",params![key,mode,full,i64::try_from(expected.checked_add(1).context("permission_revision_limit")?).context("permission_revision_limit")?])?;
        let result = permissions(&tx, &key)?;
        tx.commit()?;
        Ok(result)
    }
    /// A send only initializes a new conversation mode; subsequent changes use CAS.
    pub fn initialize_permissions(&self, scope: &Scope, mode: &str) -> Result<()> {
        ensure!(
            matches!(mode, "ask" | "read_only"),
            "invalid_permission_mode"
        );
        self.db.lock().unwrap().execute(
            "INSERT OR IGNORE INTO agent_permissions VALUES(?1,?2,0,0)",
            params![scope.key()?, mode],
        )?;
        Ok(())
    }
    pub fn create_pending(
        &self,
        scope: &Scope,
        authority: &Scope,
        run: &str,
        action: &str,
        mut value: Value,
    ) -> Result<Value> {
        ensure!(
            scope.owner == authority.owner && scope.desktop == authority.desktop,
            "authorization_scope_mismatch"
        );
        let details = value
            .as_object_mut()
            .context("invalid_pending")?
            .remove("_details");
        ensure!(
            details
                .as_ref()
                .is_none_or(|d| d.to_string().len() <= 1024 * 1024),
            "approval_details_size_limit"
        );
        ensure!(
            serde_json::to_vec(&value)?.len() <= 32768,
            "pending_size_limit"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let key = scope.key()?;
        if let Some(id) = tx
            .query_row(
                "SELECT id FROM human_pending WHERE scope=?1 AND action=?2",
                params![key, action],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return pending_value(&tx, &id);
        }
        let at = now();
        retire_pending(&tx, &key, at)?;
        let authority_key = authority.key()?;
        if authority_key != key {
            retire_pending(&tx, &authority_key, at)?;
        }
        let id = id();
        let expires = at + 24 * 60 * 60 * 1000;
        value["id"] = json!(id);
        value["state"] = json!("pending");
        value["agent_id"] = json!(scope.agent);
        value["run_id"] = json!(run);
        value["session_id"] = json!(scope.session);
        value["created_at"] = json!(at);
        value["expires_at"] = json!(expires);
        tx.execute(
            "INSERT INTO human_pending VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',NULL,?8)",
            params![
                id,
                key,
                authority.key()?,
                run,
                action,
                value["fingerprint"].as_str(),
                value.to_string(),
                expires
            ],
        )?;
        if let Some(details) = details {
            tx.execute(
                "INSERT INTO approval_details VALUES(?1,?2)",
                params![id, serde_json::to_string_pretty(&details)?],
            )?;
        }
        tx.commit()?;
        Ok(value)
    }
    pub fn approval_details(
        &self,
        scope: &Scope,
        pending: &str,
        cursor: Option<&str>,
    ) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let key = scope.key()?;
        let text:String= db.query_row("SELECT d.value FROM approval_details d JOIN human_pending p ON p.id=d.pending WHERE p.id=?1 AND (p.scope=?2 OR p.authority=?2)",params![pending,key],|r|r.get(0)).optional()?.context("pending_not_found")?;
        let item = pending_value(&db, pending)?;
        let mut offset = 0usize;
        if let Some(cursor) = cursor {
            let c: PageCursor = self.decode(cursor)?;
            ensure!(
                c.scope == key && c.kind == format!("approval_details/{pending}"),
                "cursor_scope_mismatch"
            );
            offset = c.position.parse()?;
        }
        ensure!(
            offset <= text.len() && text.is_char_boundary(offset),
            "invalid_cursor_offset"
        );
        let end = text.floor_char_boundary((offset + 8192).min(text.len()));
        let more = end < text.len();
        let cursor = if more {
            Some(self.authorization_cursor(
                &key,
                &format!("approval_details/{pending}"),
                &end.to_string(),
            )?)
        } else {
            None
        };
        Ok(
            json!({"pending_id":pending,"fingerprint":item["fingerprint"],"text":&text[offset..end],"cursor":cursor,"has_more":more,"truncated":false}),
        )
    }
    fn authorization_cursor(&self, scope: &str, kind: &str, position: &str) -> Result<String> {
        let bytes = serde_json::to_vec(&PageCursor {
            scope: scope.into(),
            kind: kind.into(),
            position: position.into(),
        })?;
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&bytes),
            blake3::keyed_hash(&self.cursor_key, &bytes).to_hex()
        ))
    }
    pub fn pending(&self, scope: &Scope, cursor: Option<&str>) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let key = scope.key()?;
        retire_pending(&db, &key, now())?;
        let mut stmt=db.prepare("SELECT id FROM human_pending WHERE (scope=?1 OR authority=?1) AND state != 'consumed' AND (?2='' OR id<?2) ORDER BY id DESC LIMIT 65")?;
        let before = if let Some(cursor) = cursor {
            let c: PageCursor = self.decode(cursor)?;
            ensure!(
                c.scope == key && c.kind == "pending",
                "cursor_scope_mismatch"
            );
            c.position
        } else {
            String::new()
        };
        let mut ids = stmt
            .query_map(params![key, before], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = ids.len() > 64;
        ids.truncate(64);
        let cursor = if more {
            Some(self.authorization_cursor(&key, "pending", ids.last().unwrap())?)
        } else {
            None
        };
        let items = ids
            .iter()
            .map(|id| pending_value(&db, id))
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"items":items,"cursor":cursor,"has_more":more}))
    }
    pub fn resolve_pending(
        &self,
        scope: &Scope,
        request: &str,
        id: &str,
        decision: Option<&str>,
        answer: Option<Value>,
    ) -> Result<Value> {
        self.resolve_pending_ack(scope, request, id, decision, answer, None, false)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_pending_ack(
        &self,
        scope: &Scope,
        request: &str,
        id: &str,
        decision: Option<&str>,
        answer: Option<Value>,
        fingerprint: Option<&str>,
        details_ack: bool,
    ) -> Result<Value> {
        ensure!(
            !request.is_empty() && request.len() <= 128,
            "invalid_request_id"
        );
        ensure!(
            decision.is_some() != answer.is_some(),
            "pending_response_type_required"
        );
        let payload = json!({"pending_id":id,"decision":decision,"answer":answer,"fingerprint":fingerprint,"details_ack":details_ack});
        ensure!(payload.to_string().len() <= 8192, "answer_size_limit");
        let hash = blake3::hash(payload.to_string().as_bytes())
            .to_hex()
            .to_string();
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let key = scope.key()?;
        if let Some((old, result)) = tx
            .query_row(
                "SELECT hash,value FROM human_responses WHERE scope=?1 AND request=?2",
                params![key, request],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure!(old == hash, "request_id_conflict");
            let mut result: Value = serde_json::from_str(&result)?;
            result["duplicate"] = json!(true);
            return Ok(result);
        }
        let allowed: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM human_pending WHERE id=?1 AND (scope=?2 OR authority=?2))",
            params![id, key],
            |r| r.get(0),
        )?;
        ensure!(allowed, "pending_not_found");
        let mut item = pending_value(&tx, id)?;
        if matches!(item["state"].as_str(), Some("resolved" | "consumed")) {
            ensure!(
                item["response"]["decision"] == json!(decision)
                    && item["response"]["answer"] == json!(answer),
                "pending_already_resolved"
            );
            ensure!(
                fingerprint.is_none_or(|f| item["fingerprint"] == f),
                "pending_fingerprint_mismatch"
            );
            let result = json!({"pending":item,"duplicate":true});
            tx.execute(
                "INSERT INTO human_responses VALUES(?1,?2,?3,?4)",
                params![key, request, hash, result.to_string()],
            )?;
            tx.commit()?;
            return Ok(result);
        }
        ensure!(item["state"] == "pending", "pending_already_resolved");
        ensure!(
            item["expires_at"].as_i64().unwrap_or(0) > now(),
            "pending_expired"
        );
        if let Some(fingerprint) = fingerprint {
            ensure!(
                item["fingerprint"] == fingerprint,
                "pending_fingerprint_mismatch"
            );
        }
        if item["kind"] == "approval" {
            if decision != Some("deny") && item["requires_details"] == true {
                ensure!(
                    details_ack && fingerprint == item["fingerprint"].as_str(),
                    "approval_details_required"
                );
            }
            ensure!(
                matches!(decision, Some("once" | "always" | "deny")) && answer.is_none(),
                "invalid_approval_decision"
            );
            ensure!(
                decision != Some("always") || item["can_always"] == true,
                "permanent_rule_unavailable"
            );
        } else {
            ensure!(
                decision.is_none() && answer.as_ref().is_some_and(Value::is_string),
                "invalid_question_answer"
            );
        }
        tx.execute(
            "UPDATE human_pending SET state='resolved',response=?2 WHERE id=?1",
            params![id, payload.to_string()],
        )?;
        item["state"] = json!("resolved");
        item["response"] = payload;
        let result = json!({"pending":item,"duplicate":false});
        tx.execute(
            "INSERT INTO human_responses VALUES(?1,?2,?3,?4)",
            params![key, request, hash, result.to_string()],
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn poll_pending(&self, scope: &Scope, id: &str) -> Result<Value> {
        let db = self.db.lock().unwrap();
        ensure!(
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM human_pending WHERE id=?1 AND scope=?2)",
                params![id, scope.key()?],
                |r| r.get::<_, bool>(0)
            )?,
            "pending_not_found"
        );
        db.execute("UPDATE human_pending SET state='expired',value=json_set(value,'$.terminal_at',expires) WHERE id=?1 AND state IN ('pending','resolved') AND expires<=?2",params![id,now()])?;
        pending_value(&db, id)
    }
    /// Atomically consumes the exact response once; approval is never an action replay API.
    pub fn consume_pending(&self, scope: &Scope, id: &str) -> Result<Value> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let item = pending_value(&tx, id)?;
        ensure!(
            item["expires_at"].as_i64().unwrap_or(0) > now(),
            "pending_expired"
        );
        ensure!(tx.execute("UPDATE human_pending SET state='consumed' WHERE id=?1 AND scope=?2 AND state='resolved'",params![id,scope.key()?])?==1,"pending_not_resolved");
        if item["response"]["decision"] == "always" {
            let fingerprint = item["fingerprint"]
                .as_str()
                .context("missing_fingerprint")?;
            let rule_id=tx.query_row("SELECT id FROM authorization_rules WHERE owner=?1 AND desktop=?2 AND fingerprint=?3",params![scope.owner,scope.desktop,fingerprint],|r|r.get::<_,String>(0)).optional()?.unwrap_or_else(||id.into());
            let rule = json!({"id":rule_id,"fingerprint":fingerprint,"created_at":now(),"tool":item["tool"],"cwd":item["cwd"],"arguments_preview":item["arguments_preview"],"rule_preview":item["rule_preview"]});
            tx.execute("INSERT INTO authorization_rules VALUES(?1,?2,?3,?4,?5,0) ON CONFLICT(owner,desktop,fingerprint) DO UPDATE SET revoked=0,value=excluded.value",params![id,scope.owner,scope.desktop,fingerprint,rule.to_string()])?;
        }
        tx.commit()?;
        Ok(item["response"].clone())
    }
    pub fn supersede_approval(&self, scope: &Scope, id: &str) -> Result<()> {
        ensure!(self.db.lock().unwrap().execute("UPDATE human_pending SET state='superseded',value=json_set(value,'$.terminal_at',?3) WHERE id=?1 AND scope=?2 AND state='pending' AND json_extract(value,'$.kind')='approval'",params![id,scope.key()?,now()])? == 1,"pending_not_available");
        Ok(())
    }
    pub fn cancel_pending_run(&self, scope: &Scope, run: &str) -> Result<()> {
        self.db.lock().unwrap().execute("UPDATE human_pending SET state='cancelled',value=json_set(value,'$.terminal_at',?3) WHERE scope=?1 AND run=?2 AND state IN ('pending','resolved')",params![scope.key()?,run,now()])?;
        Ok(())
    }
    pub fn action_denied(&self, scope: &Scope, run: &str, fingerprint: &str) -> Result<bool> {
        Ok(self.db.lock().unwrap().query_row("SELECT EXISTS(SELECT 1 FROM human_pending WHERE scope=?1 AND run=?2 AND fingerprint=?3 AND json_extract(response,'$.decision')='deny')",params![scope.key()?,run,fingerprint],|r|r.get(0))?)
    }
    pub fn rule_matches(&self, scope: &Scope, fingerprint: &str) -> Result<bool> {
        Ok(self.db.lock().unwrap().query_row("SELECT EXISTS(SELECT 1 FROM authorization_rules WHERE owner=?1 AND desktop=?2 AND fingerprint=?3 AND revoked=0)",params![scope.owner,scope.desktop,fingerprint],|r|r.get(0))?)
    }
    pub fn rules(&self, scope: &Scope, cursor: Option<&str>) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let mut stmt=db.prepare("SELECT id,value FROM authorization_rules WHERE owner=?1 AND desktop=?2 AND revoked=0 AND id>?3 ORDER BY id LIMIT 65")?;
        let key = serde_json::to_string(&json!({"owner":scope.owner,"desktop":scope.desktop}))?;
        let after = if let Some(cursor) = cursor {
            let c: PageCursor = self.decode(cursor)?;
            ensure!(c.scope == key && c.kind == "rules", "cursor_scope_mismatch");
            c.position
        } else {
            String::new()
        };
        let mut rows = stmt
            .query_map(params![scope.owner, scope.desktop, after], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = rows.len() > 64;
        rows.truncate(64);
        let cursor = if more {
            Some(self.authorization_cursor(&key, "rules", &rows.last().unwrap().0)?)
        } else {
            None
        };
        let items = rows
            .iter()
            .map(|r| serde_json::from_str::<Value>(&r.1))
            .collect::<serde_json::Result<Vec<_>>>()?;
        Ok(json!({"items":items,"cursor":cursor,"has_more":more}))
    }
    pub fn revoke_rule_request(&self, scope: &Scope, request: &str, rule: &str) -> Result<Value> {
        ensure!(
            !request.is_empty() && request.len() <= 128,
            "invalid_request_id"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let key = scope.key()?;
        let hash = blake3::hash(format!("revoke_rule/{rule}").as_bytes())
            .to_hex()
            .to_string();
        if let Some((old, value)) = tx
            .query_row(
                "SELECT hash,value FROM human_responses WHERE scope=?1 AND request=?2",
                params![key, request],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure!(old == hash, "request_id_conflict");
            return Ok(serde_json::from_str(&value)?);
        }
        ensure!(
            tx.execute(
                "UPDATE authorization_rules SET revoked=1 WHERE id=?1 AND owner=?2 AND desktop=?3",
                params![rule, scope.owner, scope.desktop]
            )? == 1,
            "rule_not_found"
        );
        let result = json!({"revoked":true,"rule_id":rule});
        tx.execute(
            "INSERT INTO human_responses VALUES(?1,?2,?3,?4)",
            params![key, request, hash, result.to_string()],
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn revoke_rule(&self, scope: &Scope, rule: &str) -> Result<Value> {
        let db = self.db.lock().unwrap();
        ensure!(
            db.execute(
                "UPDATE authorization_rules SET revoked=1 WHERE id=?1 AND owner=?2 AND desktop=?3",
                params![rule, scope.owner, scope.desktop]
            )? == 1,
            "rule_not_found"
        );
        Ok(json!({"revoked":true,"rule_id":rule}))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageCursor {
    scope: String,
    kind: String,
    position: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> (tempfile::TempDir, Store, Scope) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("private/agent.db")).unwrap();
        let scope = store.agent("owner", "desktop", None).unwrap();
        (dir, store, scope)
    }
    fn approval(store: &Store, scope: &Scope, action: &str, fingerprint: &str) -> Value {
        store.create_pending(scope,scope,"run",action,json!({"kind":"approval","tool":"run_command","fingerprint":fingerprint,"can_always":true,"arguments_preview":"/usr/bin/printf value","cwd":"/actual/cwd","_details":{"command":"/usr/bin/printf value"}})).unwrap()
    }
    #[test]
    fn retired_cards_bound_thousands_of_failures_without_dropping_live_or_retry_receipts() {
        let (_dir, store, scope) = store();
        let child = store.agent("owner", "desktop", Some("child")).unwrap();
        let old = approval(&store, &scope, "old", "old-fingerprint");
        let old_id = old["id"].as_str().unwrap();
        store
            .resolve_pending(&scope, "lost-ack", old_id, Some("once"), None)
            .unwrap();
        store.cancel_pending_run(&scope, "run").unwrap();
        let mut last_id = String::new();
        for index in 0..1001 {
            let target = if index % 2 == 0 { &scope } else { &child };
            let run = format!("cancel-{index}");
            let card = store
                .create_pending(
                    target,
                    &scope,
                    &run,
                    &run,
                    json!({"kind":"approval","_details":{"command":"example"}}),
                )
                .unwrap();
            last_id = card["id"].as_str().unwrap().to_owned();
            store.cancel_pending_run(target, &run).unwrap();
        }
        let live = store
            .create_pending(
                &child,
                &scope,
                "live",
                "live",
                json!({"kind":"question","question":"Current question?"}),
            )
            .unwrap();
        let resolved = store
            .create_pending(
                &scope,
                &scope,
                "resolved",
                "resolved",
                json!({"kind":"question","question":"Answered"}),
            )
            .unwrap();
        store
            .resolve_pending(
                &scope,
                "answer",
                resolved["id"].as_str().unwrap(),
                None,
                Some(json!("answer")),
            )
            .unwrap();
        let mut cursor = None;
        let mut rows = Vec::new();
        loop {
            let page = store.pending(&scope, cursor.as_deref()).unwrap();
            rows.extend(page["items"].as_array().unwrap().clone());
            cursor = page["cursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(rows.len(), 66);
        assert!(
            rows.iter()
                .any(|v| v["id"] == live["id"] && v["state"] == "pending")
        );
        assert!(
            rows.iter()
                .any(|v| v["id"] == resolved["id"] && v["state"] == "resolved")
        );
        assert!(rows.iter().any(|v| v["id"] == last_id));
        assert!(store.approval_details(&scope, old_id, None).is_err());
        assert_eq!(
            store
                .resolve_pending(&scope, "lost-ack", old_id, Some("once"), None)
                .unwrap()["duplicate"],
            true
        );
        assert_eq!(
            store
                .consume_pending(&scope, resolved["id"].as_str().unwrap())
                .unwrap()["answer"],
            "answer"
        );
        assert!(
            store
                .poll_pending(&child, live["id"].as_str().unwrap())
                .is_ok()
        );
    }
    #[test]
    fn terminal_ttl_and_scope_are_independent_of_actionable_deadlines() {
        let (_dir, store, scope) = store();
        let foreign = store.agent("other", "desktop", None).unwrap();
        let aged = approval(&store, &scope, "aged", "aged");
        store.cancel_pending_run(&scope, "run").unwrap();
        let untouched = approval(&store, &foreign, "foreign", "foreign");
        store.cancel_pending_run(&foreign, "run").unwrap();
        let at = now();
        store
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE human_pending SET value=json_set(value,'$.terminal_at',?1)",
                [at - TERMINAL_PENDING_TTL - 1],
            )
            .unwrap();
        let fresh = approval(&store, &scope, "fresh", "fresh");
        let rows = store.pending(&scope, None).unwrap();
        assert_eq!(rows["items"].as_array().unwrap().len(), 1);
        assert_eq!(rows["items"][0]["id"], fresh["id"]);
        assert!(
            store
                .poll_pending(&scope, aged["id"].as_str().unwrap())
                .is_err()
        );
        assert!(
            store
                .poll_pending(&foreign, untouched["id"].as_str().unwrap())
                .is_ok()
        );
    }
    #[test]
    fn permission_cas_never_promotes_old_input_or_readonly_full() {
        let (_dir, store, scope) = store();
        store.initialize_permissions(&scope, "ask").unwrap();
        assert_eq!(
            store.permissions(&scope).unwrap()["full_authorization"],
            false
        );
        store.set_permissions(&scope, 0, None, Some(true)).unwrap();
        assert!(store.set_permissions(&scope, 0, None, Some(false)).is_err());
        let permissions = store
            .set_permissions(&scope, 1, Some("read_only"), Some(true))
            .unwrap();
        assert_eq!(permissions["full_authorization"], false);
        assert_eq!(permissions["revision"], 2);
        let other = store.agent("other", "desktop", None).unwrap();
        assert_eq!(store.permissions(&other).unwrap()["revision"], 0);
    }
    #[test]
    fn approval_is_exact_once_nonce_bound_and_scope_checked() {
        let (_dir, store, scope) = store();
        let item = approval(&store, &scope, "action", "fingerprint1");
        let id = item["id"].as_str().unwrap();
        let other = store.agent("other", "desktop", None).unwrap();
        assert!(
            store
                .resolve_pending(&other, "r", id, Some("once"), None)
                .is_err()
        );
        let result = store
            .resolve_pending(&scope, "r", id, Some("once"), None)
            .unwrap();
        assert_eq!(result["duplicate"], false);
        assert_eq!(
            store
                .resolve_pending(&scope, "r", id, Some("once"), None)
                .unwrap()["duplicate"],
            true
        );
        assert!(
            store
                .resolve_pending(&scope, "r", id, Some("deny"), None)
                .is_err()
        );
        assert_eq!(
            store.consume_pending(&scope, id).unwrap()["decision"],
            "once"
        );
        assert!(store.consume_pending(&scope, id).is_err());
        assert!(!store.rule_matches(&scope, "fingerprint1").unwrap());
    }
    #[test]
    fn always_revoke_regrant_retains_consistent_rule_id() {
        let (_dir, store, scope) = store();
        let mut initial = String::new();
        for n in 0..2 {
            let item = approval(&store, &scope, &format!("action{n}"), "exact");
            let id = item["id"].as_str().unwrap();
            store
                .resolve_pending(&scope, &format!("response{n}"), id, Some("always"), None)
                .unwrap();
            store.consume_pending(&scope, id).unwrap();
            assert!(store.rule_matches(&scope, "exact").unwrap());
            assert!(!store.rule_matches(&scope, "different-cwd-or-args").unwrap());
            let rules = store.rules(&scope, None).unwrap();
            let rule = rules["items"][0]["id"].as_str().unwrap();
            if n == 0 {
                initial = rule.into();
            } else {
                assert_eq!(rule, initial);
            }
            store.revoke_rule(&scope, rule).unwrap();
            assert!(!store.rule_matches(&scope, "exact").unwrap());
        }
    }
    #[test]
    fn deny_is_remembered_only_for_that_run_and_exact_fingerprint() {
        let (_dir, store, scope) = store();
        let item = approval(&store, &scope, "a", "denied");
        let id = item["id"].as_str().unwrap();
        store
            .resolve_pending(&scope, "r", id, Some("deny"), None)
            .unwrap();
        store.consume_pending(&scope, id).unwrap();
        assert!(store.action_denied(&scope, "run", "denied").unwrap());
        assert!(!store.action_denied(&scope, "new-run", "denied").unwrap());
        assert!(!store.action_denied(&scope, "run", "other").unwrap());
    }
    #[test]
    fn global_can_answer_delegated_question_without_other_scope_access() {
        let (_dir, store, root) = store();
        let child = store.agent("owner", "desktop", Some("terminal")).unwrap();
        let independent = store
            .create_global("owner", "desktop", "independent")
            .unwrap();
        let item = store
            .create_pending(
                &child,
                &root,
                "childrun",
                "q",
                json!({"kind":"question","question":"Which target?","options":["A","B"]}),
            )
            .unwrap();
        let id = item["id"].as_str().unwrap();
        assert_eq!(
            store.pending(&root, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .resolve_pending(&independent, "r", id, None, Some(json!("A")))
                .is_err()
        );
        assert!(
            store
                .resolve_pending(&root, "r", id, Some("once"), None)
                .is_err()
        );
        assert!(
            store
                .resolve_pending(&root, "r", id, None, Some(json!({"answer":"A"})))
                .is_err()
        );
        store
            .resolve_pending(&root, "r", id, None, Some(json!("A")))
            .unwrap();
        assert_eq!(store.consume_pending(&child, id).unwrap()["answer"], "A");
        assert!(store.poll_pending(&root, id).is_err());
    }
    #[test]
    fn long_details_are_private_paginated_scope_bound_and_acknowledged() {
        let (_dir, store, scope) = store();
        let command = format!("/usr/bin/printf {}", "中文target".repeat(1500));
        let item=store.create_pending(&scope,&scope,"run","long",json!({"kind":"approval","fingerprint":"exact","can_always":true,"requires_details":true,"arguments_preview":"preview","_details":{"command":command}})).unwrap();
        assert!(item.get("_details").is_none());
        let id = item["id"].as_str().unwrap();
        assert!(
            store
                .resolve_pending(&scope, "r", id, Some("once"), None)
                .is_err()
        );
        let mut full = String::new();
        let mut cursor = None;
        loop {
            let page = store
                .approval_details(&scope, id, cursor.as_deref())
                .unwrap();
            assert!(page["text"].as_str().unwrap().len() <= 8192);
            full.push_str(page["text"].as_str().unwrap());
            cursor = page["cursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            serde_json::from_str::<Value>(&full).unwrap()["command"],
            command
        );
        let other = store.agent("other", "desktop", None).unwrap();
        assert!(store.approval_details(&other, id, None).is_err());
        store
            .resolve_pending_ack(&scope, "r", id, Some("once"), None, Some("exact"), true)
            .unwrap();
    }
    #[test]
    fn restart_interrupts_pending_but_keeps_modes_and_rules() {
        let (dir, store, scope) = store();
        store.set_permissions(&scope, 0, None, Some(true)).unwrap();
        let item = approval(&store, &scope, "a", "rule");
        let id = item["id"].as_str().unwrap().to_owned();
        store
            .resolve_pending(&scope, "r", &id, Some("always"), None)
            .unwrap();
        store.consume_pending(&scope, &id).unwrap();
        let item = approval(&store, &scope, "waiting", "pending");
        let pending = item["id"].as_str().unwrap().to_owned();
        drop(store);
        let restored = Store::open(&dir.path().join("private/agent.db")).unwrap();
        assert_eq!(
            restored.permissions(&scope).unwrap()["full_authorization"],
            true
        );
        assert!(restored.rule_matches(&scope, "rule").unwrap());
        assert_eq!(
            restored.poll_pending(&scope, &pending).unwrap()["state"],
            "interrupted"
        );
        assert!(
            restored
                .resolve_pending(&scope, "old", &pending, Some("once"), None)
                .is_err()
        );
    }
    #[test]
    fn cancellation_and_expiry_never_consume_old_answers() {
        let (_dir, store, scope) = store();
        let item = approval(&store, &scope, "a", "cancelled");
        let id = item["id"].as_str().unwrap();
        store.cancel_pending_run(&scope, "run").unwrap();
        assert_eq!(
            store.poll_pending(&scope, id).unwrap()["state"],
            "cancelled"
        );
        assert!(
            store
                .resolve_pending(&scope, "r", id, Some("once"), None)
                .is_err()
        );
        let item = approval(&store, &scope, "b", "expired");
        let id = item["id"].as_str().unwrap();
        store
            .db
            .lock()
            .unwrap()
            .execute("UPDATE human_pending SET expires=0 WHERE id=?1", [id])
            .unwrap();
        assert_eq!(store.poll_pending(&scope, id).unwrap()["state"], "expired");
        assert!(
            store
                .resolve_pending(&scope, "r2", id, Some("once"), None)
                .is_err()
        );
    }
}
