//! Scoped, bounded metadata cache. Cursors refer to one captured provider page.
use ai_terminal_agent_runtime::{
    catalog::{CatalogModel, CatalogPage},
    config::Provider,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
#[derive(Serialize, Deserialize)]
struct Cached {
    identity: String,
    at: i64,
    page: CatalogPage,
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    identity: String,
    cache: String,
    offset: usize,
    search: String,
}
#[derive(Serialize, Deserialize)]
pub struct Page {
    pub models: Vec<CatalogModel>,
    pub cursor: Option<String>,
    pub fetched_at: i64,
    pub stale: bool,
    pub cached: bool,
    pub provider_revision: u64,
}
pub struct Catalog {
    root: PathBuf,
    lock: Mutex<()>,
}
impl Catalog {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.join("data/model-catalog"),
            lock: Mutex::new(()),
        }
    }
    #[allow(clippy::too_many_arguments)] // The lookup binds credentials and all pagination parameters.
    pub fn discover(
        &self,
        owner: &str,
        revision: u64,
        provider: &Provider,
        secret: &str,
        search: &str,
        cursor: Option<&str>,
        refresh: bool,
    ) -> Result<Page> {
        ensure!(search.len() <= 256, "catalog_search_limit");
        let _guard = self.lock.lock().unwrap();
        crate::service::secure_dir(&self.root)?;
        let identity = blake3::hash(&serde_json::to_vec(&(owner, revision, provider))?)
            .to_hex()
            .to_string();
        let parsed: Option<Cursor> = cursor
            .map(|c| {
                ensure!(c.len() <= 4096, "invalid_catalog_cursor");
                Ok::<_, anyhow::Error>(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(c)?)?)
            })
            .transpose()?;
        if let Some(c) = &parsed {
            ensure!(
                c.identity == identity && c.search == search,
                "catalog_cursor_scope_mismatch"
            );
            ensure!(
                c.cache.len() == 64 && c.cache.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid_catalog_cursor"
            );
        }
        let cache = parsed
            .as_ref()
            .map_or_else(|| identity.clone(), |c| c.cache.clone());
        let path = self.root.join(format!("{cache}.json"));
        let mut was_cached = true;
        let saved = if path.exists() && !refresh {
            ensure!(
                std::fs::metadata(&path)?.len() <= 2 * 1024 * 1024,
                "catalog_cache_limit"
            );
            let c: Cached = serde_json::from_slice(&std::fs::read(&path)?)?;
            ensure!(c.identity == identity, "catalog_cursor_scope_mismatch");
            c
        } else {
            ensure!(parsed.is_none(), "catalog_cursor_expired");
            was_cached = false;
            self.fetch(&identity, &path, provider, secret, None)?
        };
        let mut offset = parsed.as_ref().map_or(0, |c| c.offset);
        let mut saved = saved;
        let mut cache = cache;
        if offset == usize::MAX {
            let next = saved
                .page
                .next_cursor
                .as_deref()
                .context("catalog_cursor_expired")?;
            cache = blake3::hash(format!("{identity}:{next}").as_bytes())
                .to_hex()
                .to_string();
            saved = self.fetch(
                &identity,
                &self.root.join(format!("{cache}.json")),
                provider,
                secret,
                Some(next),
            )?;
            offset = 0;
            was_cached = false;
        }
        let matches: Vec<_> = saved
            .page
            .models
            .iter()
            .filter(|m| m.id.contains(search) || m.name.contains(search))
            .cloned()
            .collect();
        ensure!(offset <= matches.len(), "invalid_catalog_cursor");
        let models: Vec<_> = matches.iter().skip(offset).take(50).cloned().collect();
        let end = offset + models.len();
        let next = if end < matches.len() {
            Some(end)
        } else {
            saved.page.next_cursor.as_ref().map(|_| usize::MAX)
        };
        let cursor = next
            .map(|offset| {
                Ok::<_, anyhow::Error>(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&Cursor {
                    identity,
                    cache,
                    offset,
                    search: search.into(),
                })?))
            })
            .transpose()?;
        Ok(Page {
            models,
            cursor,
            fetched_at: saved.at,
            stale: chrono::Utc::now().timestamp() - saved.at > 3600,
            cached: was_cached,
            provider_revision: revision,
        })
    }
    fn fetch(
        &self,
        identity: &str,
        path: &Path,
        provider: &Provider,
        secret: &str,
        cursor: Option<&str>,
    ) -> Result<Cached> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let page = runtime.block_on(ai_terminal_agent_runtime::catalog::fetch(
            provider, secret, cursor,
        ))?;
        let saved = Cached {
            identity: identity.into(),
            at: chrono::Utc::now().timestamp(),
            page,
        };
        let tmp = path.with_extension("tmp");
        let mut file = crate::service::open_private(&tmp, false)?;
        file.set_len(0)?;
        file.write_all(&serde_json::to_vec(&saved)?)?;
        file.sync_all()?;
        std::fs::rename(tmp, path)?;
        let mut files = std::fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
            .collect::<Vec<_>>();
        files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        let excess = files.len().saturating_sub(32);
        for old in files.into_iter().take(excess) {
            if old.path() != path {
                std::fs::remove_file(old.path())?;
            }
        }
        Ok(saved)
    }
}
