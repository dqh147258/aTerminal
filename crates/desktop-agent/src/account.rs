use crate::{
    Client,
    remote_bridge::{Lease, serve_channel},
};
use ai_terminal_remote::{Channel, account::AccountSession};
use ai_terminal_security::account::DesktopAccountCommand;
use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;

struct Vault {
    entry: Option<keyring::Entry>,
    fallback: PathBuf,
    file: bool,
}
impl Vault {
    fn new(dir: &Path) -> Result<Self> {
        let references = dir.join("credentials");
        crate::service::secure_dir(&references)?;
        let reference = references.join("account-reference.json");
        let id = if reference.exists() {
            let reference: crate::state::VaultReference =
                serde_json::from_slice(&std::fs::read(reference)?)?;
            anyhow::ensure!(
                reference.id.len() == 64 && reference.id.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid vault reference"
            );
            reference.id
        } else {
            let id = blake3::hash(dir.to_string_lossy().as_bytes())
                .to_hex()
                .to_string();
            // Persist the original identity once, before an account can be stored.
            // Equivalent path spellings must not choose another keyring entry.
            let temp = reference.with_extension("tmp");
            let mut file = crate::service::open_private(&temp, false)?;
            file.set_len(0)?;
            serde_json::to_writer(
                &mut file,
                &crate::state::VaultReference {
                    id: id.clone(),
                    legacy_file: None,
                },
            )?;
            file.sync_all()?;
            std::fs::rename(temp, reference)?;
            id
        };
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
            .unwrap_or_else(|| dir.to_owned());
        let file = cfg!(unix)
            && std::env::var("AI_TERMINAL_CREDENTIAL_STORE").is_ok_and(|s| s == "file")
            || cfg!(target_os = "linux") && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none();
        let fallback = references.join("account.json");
        let legacy = root.join("ai-terminal").join(format!("{id}.json"));
        if file && !fallback.exists() && legacy.exists() {
            // Preserve existing file-backend identity without changing a keyring backend.
            let legacy_vault = Self {
                entry: None,
                fallback: legacy,
                file: true,
            };
            if let Some(session) = legacy_vault.load()? {
                Self {
                    entry: None,
                    fallback: fallback.clone(),
                    file: true,
                }
                .save(&session)?;
            }
        }
        Ok(Self {
            entry: if file {
                None
            } else {
                Some(keyring::Entry::new("dev.aiterminal.account", &id)?)
            },
            fallback,
            file,
        })
    }
    fn load(&self) -> Result<Option<AccountSession>> {
        let value = if self.file {
            if let Ok(meta) = std::fs::symlink_metadata(&self.fallback) {
                anyhow::ensure!(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "credential file must be a regular private file"
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    anyhow::ensure!(
                        meta.permissions().mode() & 0o077 == 0,
                        "credential file must have mode 0600"
                    );
                }
                crate::service::secure_dir(self.fallback.parent().unwrap())?;
            }
            match std::fs::read_to_string(&self.fallback) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        } else {
            match self.entry.as_ref().unwrap().get_password() {
                Ok(s) => s,
                Err(keyring::Error::NoEntry) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        };
        Ok(Some(serde_json::from_str(&value)?))
    }
    fn save(&self, session: &AccountSession) -> Result<()> {
        let value = serde_json::to_string(session)?;
        if self.file {
            let parent = self.fallback.parent().unwrap();
            crate::service::secure_dir(parent)?;
            let tmp = self.fallback.with_extension("new");
            use std::io::Write;
            let mut file = crate::service::open_private(&tmp, false)?;
            file.set_len(0)?;
            file.write_all(value.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(tmp, &self.fallback)?;
        } else {
            self.entry.as_ref().unwrap().set_password(&value)?;
        }
        Ok(())
    }
    fn remove(&self) -> Result<()> {
        if self.file {
            match std::fs::remove_file(&self.fallback) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        } else {
            match self.entry.as_ref().unwrap().delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(e.into()),
            }
        };
        Ok(())
    }
}
pub(crate) struct AccountManager {
    state: Mutex<Option<AccountSession>>,
    vault: Vault,
    dir: PathBuf,
    generation: AtomicU64,
    owner: std::sync::Mutex<String>,
    load_error: Option<String>,
}
impl AccountManager {
    pub fn new(dir: &Path) -> Result<Arc<Self>> {
        crate::service::startup_stage("account:vault-start");
        let vault = Vault::new(dir)?;
        crate::service::startup_stage("account:vault-ready");
        crate::service::startup_stage("account:load-start");
        let (state, load_error) = match vault.load() {
            Ok(s) => (s, None),
            Err(e) => (None, Some(e.to_string())),
        };
        crate::service::startup_stage("account:load-ready");
        let owner = state.as_ref().map(principal).unwrap_or_else(|| {
            std::fs::read_to_string(vault.fallback.with_extension("mode"))
                .or_else(|_| std::fs::read_to_string(dir.join("account-mode")))
                .unwrap_or_default()
        });
        if !owner.is_empty() {
            write_marker(&dir.join("account-mode"), &owner)?;
        }
        Ok(Arc::new(Self {
            state: Mutex::new(state),
            vault,
            dir: dir.into(),
            generation: AtomicU64::new(0),
            owner: std::sync::Mutex::new(owner),
            load_error,
        }))
    }
    pub fn owner(&self) -> String {
        self.owner.lock().unwrap().clone()
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub async fn verify_device(&self, owner: &str, device: &str) -> Result<()> {
        anyhow::ensure!(self.owner() == owner, "account_changed");
        if device.is_empty() {
            return Ok(());
        }
        if let Some(pair) = device.strip_prefix("pair/") {
            anyhow::ensure!(
                !self.dir.join("account-mode").exists()
                    && pair
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
                "pair_revoked"
            );
            let bytes = std::fs::read(self.dir.join("pairs").join(format!("{pair}.json")))
                .context("pair_revoked")?;
            let pair: ai_terminal_remote::HostPair = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(
                pair.expires_at
                    > std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                "pair_expired"
            );
            return Ok(());
        }
        let mut state = self.state.lock().await;
        let session = state.as_mut().context("account_logged_out")?;
        let before = session.tokens.access_token.clone();
        let result = session.devices().await;
        if session.tokens.access_token != before {
            self.vault.save(session)?;
        }
        anyhow::ensure!(result?.iter().any(|d| d.id == device), "device_revoked");
        Ok(())
    }
    pub fn call(&self, json: &str) -> Result<String> {
        let command: DesktopAccountCommand = serde_json::from_str(json)?;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(self.command(command))
    }
    async fn command(&self, command: DesktopAccountCommand) -> Result<String> {
        let mut state = self.state.lock().await;
        match command {
            DesktopAccountCommand::Status => Ok(match state.as_ref() {
                Some(s) => format!(
                    "{} · {} · device {} · credentials: {}",
                    s.tokens.username,
                    s.server,
                    s.tokens.device_id,
                    if self.vault.file {
                        "private file"
                    } else {
                        "OS credential vault"
                    }
                ),
                None => self
                    .load_error
                    .as_ref()
                    .map(|e| format!("Credentials unavailable; local terminals remain usable: {e}"))
                    .unwrap_or_else(|| "Not logged in".into()),
            }),
            DesktopAccountCommand::Login {
                server,
                ca,
                username,
                password,
                device_name,
            } => {
                if state.is_some() {
                    bail!("log out before switching accounts")
                }
                if self.load_error.is_some() {
                    bail!(
                        "credential vault unavailable at startup; unlock it and restart the Agent before logging in"
                    )
                }
                let session = AccountSession::login(
                    &server,
                    ca,
                    username,
                    password,
                    device_name,
                    "desktop".into(),
                )
                .await?;
                self.vault.save(&session)?;
                let owner = principal(&session);
                crate::service::secure_dir(self.vault.fallback.parent().unwrap())?;
                write_marker(&self.vault.fallback.with_extension("mode"), &owner)?;
                write_marker(&self.dir.join("account-mode"), &owner)?;
                *self.owner.lock().unwrap() = owner;
                let message = format!(
                    "Logged in as {} · device {}",
                    session.tokens.username, session.tokens.device_id
                );
                *state = Some(session);
                self.generation.fetch_add(1, Ordering::AcqRel);
                Ok(message)
            }
            DesktopAccountCommand::Logout => {
                // Preserve the account-mode marker so old invitations cannot restore access.
                let result = if let Some(s) = state.as_mut() {
                    s.logout().await
                } else {
                    Ok(())
                };
                self.generation.fetch_add(1, Ordering::AcqRel);
                self.vault.remove()?;
                *state = None;
                result.context("local logout completed; server unavailable, revoke this device from another logged-in device")?;
                Ok("Logged out; local terminal sessions retained".into())
            }
            DesktopAccountCommand::Devices => {
                let s = state.as_mut().context("not logged in")?;
                let result = s.devices().await;
                self.vault.save(s)?;
                Ok(serde_json::to_string_pretty(&result?)?)
            }
            DesktopAccountCommand::Revoke { device_id } => {
                let s = state.as_mut().context("not logged in")?;
                let result = s.revoke(&device_id).await;
                // A failed action may already have rotated the refresh token.
                self.vault.save(s)?;
                result?;
                if device_id == s.tokens.device_id {
                    self.generation.fetch_add(1, Ordering::AcqRel);
                    self.vault.remove()?;
                    *state = None
                }
                Ok("Device revoked".into())
            }
        }
    }
    async fn heartbeat_persisted(
        &self,
        session: &mut AccountSession,
        needs_persist: &mut bool,
    ) -> Result<Vec<ai_terminal_security::account::ConnectionGrant>> {
        let old_access = session.tokens.access_token.clone();
        let old_pins = session.pins.len();
        let mut result = session.heartbeat().await;
        if let Ok(grants) = &mut result {
            grants.retain(|grant| session.pin(&grant.mobile_id, &grant.mobile_public).is_ok());
        }
        *needs_persist |=
            old_access != session.tokens.access_token || old_pins != session.pins.len();
        // Persist rotation even when the HTTP request after refresh failed. On storage failure
        // retain the dirty flag and do not start remote jobs until a later save succeeds.
        if *needs_persist {
            self.vault.save(session)?;
            *needs_persist = false;
        }
        result
    }
    pub fn spawn(self: Arc<Self>, stop: Arc<AtomicBool>) {
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("account runtime");
            runtime.block_on(async {
                let mut jobs: std::collections::HashMap<String, tokio::task::JoinHandle<()>> =
                    std::collections::HashMap::new();
                let mut generation = self.generation.load(Ordering::Acquire);
                let mut needs_persist = false;
                while !stop.load(Ordering::Acquire) {
                    let current = self.generation.load(Ordering::Acquire);
                    if current != generation {
                        for (_, job) in jobs.drain() {
                            job.abort()
                        }
                        generation = current;
                    }
                    let mut state = self.state.lock().await;
                    if let Some(session) = state.as_mut() {
                        match self.heartbeat_persisted(session, &mut needs_persist).await {
                            Ok(grants) => {
                                for grant in grants {
                                    if jobs.contains_key(&grant.room) {
                                        continue;
                                    }
                                    let identity = session.clone();
                                    let dir = self.dir.clone();
                                    let room = grant.room.clone();
                                    jobs.insert(
                                        room,
                                        tokio::spawn(async move {
                                            let result = async {
                                                let channel = Channel::account(
                                                    &identity,
                                                    &grant,
                                                    false,
                                                    &ice_servers(),
                                                )
                                                .await?;
                                                let mut client = Client::connect(&dir)?;
                                                client.account_scope = principal(&identity);
                                                client.device_scope = grant.mobile_id.clone();
                                                let _lease = Lease(client.clone());
                                                serve_channel(channel, client, grant.read_only)
                                                    .await
                                            }
                                            .await;
                                            if result.is_err() {
                                                eprintln!("account remote connection ended");
                                            }
                                        }),
                                    );
                                }
                            }
                            Err(_) => {
                                for (_, job) in jobs.drain() {
                                    job.abort()
                                }
                            }
                        }
                    }
                    drop(state);
                    jobs.retain(|_, job| !job.is_finished());
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                for (_, job) in jobs {
                    job.abort()
                }
            });
        });
    }
}
fn write_marker(path: &Path, value: &str) -> Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "invalid account marker"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            anyhow::ensure!(
                meta.uid() == rustix::process::getuid().as_raw(),
                "account marker owner mismatch"
            );
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    use std::io::Write;
    let mut file = crate::service::open_private(path, false)?;
    file.set_len(0)?;
    file.write_all(value.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
fn ice_servers() -> Vec<String> {
    std::env::var("AI_TERMINAL_ICE_SERVERS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().into())
        .collect()
}

fn principal(session: &AccountSession) -> String {
    format!("{}|{}", session.server, session.tokens.username)
}

#[cfg(test)]
mod tests {
    #[test]
    fn vault_reference_survives_equivalent_state_directory_paths() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("state");
        crate::service::secure_dir(&root).unwrap();
        super::Vault::new(&root).unwrap();
        let path = root.join("credentials/account-reference.json");
        let first = std::fs::read(&path).unwrap();
        super::Vault::new(&root.join(".")).unwrap();
        assert_eq!(first, std::fs::read(path).unwrap());
    }

    use super::*;
    use ai_terminal_security::account::{DeviceIdentity, Tokens};
    use axum::{Json, Router, http::StatusCode, routing::post};

    #[tokio::test]
    async fn refresh_survives_failed_heartbeat_and_failed_storage_is_retried() {
        for fail_storage in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let refreshed = Tokens {
                access_token: "new-access".into(),
                refresh_token: "new-refresh".into(),
                expires_at: i64::MAX,
                device_id: "device".into(),
                username: "user".into(),
            };
            let response = refreshed.clone();
            let router = Router::new()
                .route(
                    "/v2/auth/refresh",
                    post(move || {
                        let value = response.clone();
                        async move { Json(value) }
                    }),
                )
                .route(
                    "/v2/devices/heartbeat",
                    post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let parent = dir.path().join("credentials");
            if fail_storage {
                std::fs::write(&parent, "blocked").unwrap();
            }
            let manager = AccountManager {
                state: Mutex::new(None),
                vault: Vault {
                    entry: None,
                    file: true,
                    fallback: parent.join("account.json"),
                },
                dir: dir.path().into(),
                generation: AtomicU64::new(0),
                owner: std::sync::Mutex::new(String::new()),
                load_error: None,
            };
            let mut session = AccountSession {
                server: url,
                ca: None,
                identity: DeviceIdentity::generate().unwrap(),
                tokens: Tokens {
                    access_token: "old-access".into(),
                    refresh_token: "old-refresh".into(),
                    expires_at: 0,
                    ..refreshed
                },
                pins: Default::default(),
            };
            let mut dirty = false;
            assert!(
                manager
                    .heartbeat_persisted(&mut session, &mut dirty)
                    .await
                    .is_err()
            );
            assert_eq!(session.tokens.refresh_token, "new-refresh");
            if fail_storage {
                assert!(dirty);
                std::fs::remove_file(&parent).unwrap();
                assert!(
                    manager
                        .heartbeat_persisted(&mut session, &mut dirty)
                        .await
                        .is_err()
                );
            }
            assert!(!dirty);
            assert_eq!(
                manager.vault.load().unwrap().unwrap().tokens.refresh_token,
                "new-refresh"
            );
            server.abort();
        }
    }
}
