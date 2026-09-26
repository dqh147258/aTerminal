//! Bounded Desktop history cache and streaming, read-only legacy archive import.
use crate::CoreError;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Value, json};
use std::{fmt, path::Path, sync::Mutex};
fn ffi(e: impl fmt::Display) -> CoreError {
    CoreError::InvalidFrame {
        reason: e.to_string(),
    }
}
#[derive(uniffi::Object)]
pub struct AgentCache {
    db: Mutex<Connection>,
}
#[uniffi::export]
impl AgentCache {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Self, CoreError> {
        Self::create(&path).map_err(ffi)
    }
    /// Scope is a canonical JSON array of server, account, Desktop and session.
    pub fn store_page(
        &self,
        scope: String,
        cursor: Option<String>,
        page: String,
    ) -> Result<(), CoreError> {
        self.put(&scope, cursor.as_deref(), &page).map_err(ffi)
    }
    pub fn page(&self, scope: String, cursor: Option<String>) -> Result<Option<String>, CoreError> {
        let db = self.db.lock().map_err(ffi)?;
        db.query_row(
            "SELECT body FROM pages WHERE scope=?1 AND cursor=?2",
            params![scope, cursor.unwrap_or_default()],
            |r| r.get(0),
        )
        .optional()
        .map_err(ffi)
    }
    pub fn reconcile(&self, scope: String, generation: i64) -> Result<(), CoreError> {
        let mut db = self.db.lock().map_err(ffi)?;
        let tx = db.transaction().map_err(ffi)?;
        tx.execute("INSERT INTO cache_generations VALUES(?1,?2) ON CONFLICT(scope) DO UPDATE SET generation=MAX(generation,excluded.generation)",params![scope,generation]).map_err(ffi)?;
        tx.execute(
            "DELETE FROM pages WHERE scope=?1 AND generation<>(SELECT generation FROM cache_generations WHERE scope=?1)",
            params![scope],
        )
        .map_err(ffi)?;
        tx.commit().map_err(ffi)
    }
    /// A single legacy JSON file is decoded one message at a time inside a transaction.
    /// The source file is preserved, including on error. Imported messages never enter Desktop context.
    pub fn import_legacy(&self, scope: String, path: String) -> Result<u64, CoreError> {
        self.import(&scope, Path::new(&path)).map_err(ffi)
    }
    pub fn legacy_page(&self, scope: String, before: Option<i64>) -> Result<String, CoreError> {
        let db = self.db.lock().map_err(ffi)?;
        let mut stmt=db.prepare("SELECT sequence,body FROM legacy WHERE scope=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT 51").map_err(ffi)?;
        let mut rows = stmt
            .query_map(params![scope, before.unwrap_or(i64::MAX)], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(ffi)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ffi)?;
        let more = rows.len() > 50;
        rows.truncate(50);
        let next = if more { rows.last().map(|r| r.0) } else { None };
        let items=rows.into_iter().map(|(sequence,body)|json!({"sequence":sequence,"value":serde_json::from_str::<Value>(&body).unwrap_or(Value::Null)})).collect::<Vec<_>>();
        Ok(json!({"items":items,"before":next,"has_more":more,"source":"legacy_local","read_only":true}).to_string())
    }
    pub fn legacy_scopes(
        &self,
        identity_prefix: String,
        before: Option<String>,
    ) -> Result<String, CoreError> {
        let db = self.db.lock().map_err(ffi)?;
        let mut stmt=db.prepare("SELECT scope,count,metadata FROM imports WHERE substr(scope,1,length(?1))=?1 AND scope>?2 ORDER BY scope LIMIT 50").map_err(ffi)?;
        let rows = stmt
            .query_map(params![identity_prefix, before.unwrap_or_default()], |r| {
                Ok(json!({"scope":r.get::<_,String>(0)?,"count":r.get::<_,i64>(1)?,"metadata":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(Value::Null)}))
            })
            .map_err(ffi)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ffi)?;
        Ok(json!({"items":rows}).to_string())
    }
}
impl AgentCache {
    fn create(path: &str) -> Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=3000;
        CREATE TABLE IF NOT EXISTS cache_generations(scope TEXT PRIMARY KEY,generation INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS pages(scope TEXT NOT NULL,cursor TEXT NOT NULL,generation INTEGER NOT NULL,body TEXT NOT NULL,touched INTEGER NOT NULL,PRIMARY KEY(scope,cursor));
        CREATE TABLE IF NOT EXISTS legacy(scope TEXT NOT NULL,sequence INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(scope,sequence));
        CREATE TABLE IF NOT EXISTS imports(scope TEXT PRIMARY KEY,count INTEGER NOT NULL,metadata TEXT NOT NULL DEFAULT '{}');")?;
        let has_metadata = db
            .prepare("PRAGMA table_info(imports)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|s| s == "metadata");
        if !has_metadata {
            db.execute(
                "ALTER TABLE imports ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}'",
                [],
            )?;
        }
        Ok(Self { db: Mutex::new(db) })
    }
    fn put(&self, scope: &str, cursor: Option<&str>, body: &str) -> Result<()> {
        ensure!(
            scope.len() <= 4096 && cursor.unwrap_or("").len() <= 4096 && body.len() <= 1024 * 1024,
            "cache_page_limit"
        );
        let page: Value = serde_json::from_str(body)?;
        let generation = page["generation"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("history_generation_required"))?;
        ensure!(
            page["items"].as_array().is_some_and(|a| a.len() <= 50),
            "history_page_limit"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let known: Option<i64> = tx
            .query_row(
                "SELECT generation FROM cache_generations WHERE scope=?1",
                [scope],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            known.is_none_or(|old| generation >= old),
            "history_generation_stale"
        );
        tx.execute("INSERT INTO cache_generations VALUES(?1,?2) ON CONFLICT(scope) DO UPDATE SET generation=excluded.generation",params![scope,generation])?;
        tx.execute(
            "DELETE FROM pages WHERE scope=?1 AND generation<>?2",
            params![scope, generation],
        )?;
        tx.execute("INSERT OR REPLACE INTO pages VALUES(?1,?2,?3,?4,(SELECT COALESCE(MAX(touched),0)+1 FROM pages))",params![scope,cursor.unwrap_or(""),generation,body])?;
        tx.execute("DELETE FROM pages WHERE scope=?1 AND cursor NOT IN (SELECT cursor FROM pages WHERE scope=?1 ORDER BY touched DESC LIMIT 3)",[scope])?;
        tx.execute("DELETE FROM pages WHERE scope NOT IN (SELECT scope FROM pages GROUP BY scope ORDER BY MAX(touched) DESC LIMIT 32)",[])?;
        tx.commit()?;
        Ok(())
    }
    fn import(&self, scope: &str, path: &Path) -> Result<u64> {
        ensure!(scope.len() <= 4096, "scope_limit");
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        if let Some(count) = tx
            .query_row("SELECT count FROM imports WHERE scope=?1", [scope], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            return Ok(count as u64);
        }
        let file = std::io::BufReader::new(std::fs::File::open(path)?);
        let mut deserializer = serde_json::Deserializer::from_reader(BoundedJson::new(file));
        let (count, metadata) = Archive { db: &tx, scope }.deserialize(&mut deserializer)?;
        deserializer.end()?;
        tx.execute(
            "INSERT INTO imports(scope,count,metadata) VALUES(?1,?2,?3)",
            params![scope, count as i64, metadata.to_string()],
        )?;
        tx.commit()?;
        Ok(count)
    }
}
// Bound individual JSON strings and message objects before serde allocates them.
struct BoundedJson<R> {
    inner: R,
    depth: usize,
    string: bool,
    escape: bool,
    token: usize,
    unit: usize,
}
impl<R> BoundedJson<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            depth: 0,
            string: false,
            escape: false,
            token: 0,
            unit: 0,
        }
    }
}
impl<R: std::io::Read> std::io::Read for BoundedJson<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(output)?;
        for byte in &output[..count] {
            if self.depth >= 3 {
                self.unit += 1;
            }
            if self.string {
                self.token += 1;
                if self.escape {
                    self.escape = false;
                } else if *byte == b'\\' {
                    self.escape = true;
                } else if *byte == b'"' {
                    self.string = false;
                }
            } else {
                match *byte {
                    b'"' => {
                        self.string = true;
                        self.token = 0;
                    }
                    b'{' | b'[' => {
                        self.depth += 1;
                        if self.depth == 3 {
                            self.unit = 0;
                        }
                    }
                    b'}' | b']' => {
                        self.depth = self.depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            if self.token > 1024 * 1024 || self.unit > 1024 * 1024 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "legacy_message_limit",
                ));
            }
        }
        Ok(count)
    }
}
struct Archive<'a> {
    db: &'a Connection,
    scope: &'a str,
}
impl<'de> DeserializeSeed<'de> for Archive<'_> {
    type Value = (u64, Value);
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(u64, Value), D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Archive<'_> {
    type Value = (u64, Value);
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("legacy conversation")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(u64, Value), M::Error> {
        let mut count = None;
        let mut metadata = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if key == "messages" {
                if count.is_some() {
                    return Err(de::Error::custom("duplicate messages"));
                }
                count = Some(map.next_value_seed(Messages {
                    db: self.db,
                    scope: self.scope,
                })?)
            } else if [
                "scope",
                "title",
                "deviceId",
                "sessionId",
                "deviceName",
                "updated",
            ]
            .contains(&key.as_str())
            {
                let value = map.next_value::<Value>()?;
                if value.to_string().len() > 16384 {
                    return Err(de::Error::custom("legacy_metadata_limit"));
                }
                metadata.insert(key, value);
            } else {
                map.next_value::<de::IgnoredAny>()?;
            }
        }
        count
            .map(|n| (n, Value::Object(metadata)))
            .ok_or_else(|| de::Error::custom("legacy messages missing"))
    }
}
struct Messages<'a> {
    db: &'a Connection,
    scope: &'a str,
}
impl<'de> DeserializeSeed<'de> for Messages<'_> {
    type Value = u64;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<u64, D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Messages<'_> {
    type Value = u64;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("legacy messages")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<u64, S::Error> {
        let mut count = 0;
        while let Some(message) = seq.next_element::<Value>()? {
            let body = message.to_string();
            if body.len() > 1024 * 1024 {
                return Err(de::Error::custom("legacy_message_limit"));
            }
            count += 1;
            self.db
                .execute(
                    "INSERT INTO legacy VALUES(?1,?2,?3)",
                    params![self.scope, count as i64, body],
                )
                .map_err(de::Error::custom)?;
        }
        Ok(count)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_reconciles_and_keeps_three_pages() {
        let cache = AgentCache::create(":memory:").unwrap();
        for n in 0..4 {
            cache
                .put(
                    "scope",
                    Some(&n.to_string()),
                    &json!({"generation":1,"items":[]}).to_string(),
                )
                .unwrap();
        }
        assert!(
            cache
                .page("scope".into(), Some("0".into()))
                .unwrap()
                .is_none()
        );
        cache.reconcile("scope".into(), 2).unwrap();
        assert!(
            cache
                .page("scope".into(), Some("3".into()))
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(test)]
mod migration_contracts {
    use super::*;
    use std::io::Write;
    #[test]
    fn large_legacy_archive_import_is_idempotent_paged_and_transactional() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("legacy.json");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "{{\"title\":\"Archive\",\"messages\":[").unwrap();
        for n in 0..10001 {
            if n > 0 {
                write!(file, ",").unwrap();
            }
            write!(file, "{}", json!({"id":n,"content":"中".repeat(100)})).unwrap();
        }
        write!(file, "]}}").unwrap();
        drop(file);
        let cache = AgentCache::create(":memory:").unwrap();
        assert_eq!(cache.import("legacy/account/a", &path).unwrap(), 10001);
        assert_eq!(cache.import("legacy/account/a", &path).unwrap(), 10001);
        let first: Value =
            serde_json::from_str(&cache.legacy_page("legacy/account/a".into(), None).unwrap())
                .unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 50);
        assert_eq!(first["items"][0]["sequence"], 10001);
        let next: Value = serde_json::from_str(
            &cache
                .legacy_page("legacy/account/a".into(), first["before"].as_i64())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(next["items"][0]["sequence"], 9951);
        std::fs::write(&path, b"{\"messages\":[{\"content\":\"a\"},").unwrap();
        assert!(cache.import("broken", &path).is_err());
        assert!(path.exists());
        let broken: Value =
            serde_json::from_str(&cache.legacy_page("broken".into(), None).unwrap()).unwrap();
        assert!(broken["items"].as_array().unwrap().is_empty());
        cache.reconcile("desktop".into(), 9).unwrap();
        assert!(
            cache
                .put(
                    "desktop",
                    None,
                    &json!({"generation":8,"items":[]}).to_string()
                )
                .is_err()
        );
    }
}
