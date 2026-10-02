//! Desktop is the only configuration writer. A durable candidate journal makes
//! multi-file publication recoverable; readers see one immutable revision.
use ai_terminal_agent_runtime::config::OwnerConfig;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    installation_id: String,
    revision: u64,
    owners: BTreeMap<String, OwnerConfig>,
}
pub struct ConfigService {
    root: PathBuf,
    state: Mutex<Arc<State>>,
    catalog: crate::catalog::Catalog,
    uploads: Mutex<()>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    SkillEdit {
        id: String,
        path: String,
        body: String,
        expected_revision: u64,
    },
    SkillUploadBegin {
        id: String,
        expected_revision: u64,
    },
    SkillUploadChunk {
        upload_id: String,
        path: String,
        offset: u64,
        data: String,
    },
    SkillUploadCommit {
        upload_id: String,
    },
    Show,
    Discover {
        provider: String,
        #[serde(default)]
        search: String,
        cursor: Option<String>,
        #[serde(default)]
        refresh: bool,
    },
    Validate,
    Reload,
    CredentialSet {
        name: String,
        value: String,
        expected_revision: u64,
    },
    SkillInstall {
        id: String,
        path: PathBuf,
        expected_revision: u64,
    },
    SkillFiles {
        id: String,
        files: BTreeMap<String, String>,
        expected_revision: u64,
    },
    SkillRead {
        id: String,
        #[serde(default = "skill_md")]
        path: String,
    },
    McpValidate {
        id: String,
    },
    Replace {
        expected_revision: u64,
        config: OwnerConfig,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        secrets: BTreeMap<String, String>,
    },
}
fn skill_md() -> String {
    "SKILL.md".into()
}
#[derive(Serialize, Deserialize)]
pub struct View {
    pub schema_version: u32,
    pub installation_id: String,
    pub revision: u64,
    pub config: OwnerConfig,
}
impl ConfigService {
    pub fn open(root: &Path) -> Result<Self> {
        crate::service::secure_dir(root)?;
        let journal = root.join("config.pending.json");
        let state = if journal.exists() {
            let state: State = read_json(&journal)?;
            validate(&state)?;
            if let Ok(published) = read_json::<Root>(&root.join("config.json")) {
                ensure!(
                    published.installation_id == state.installation_id
                        && published.revision <= state.revision,
                    "stale_config_journal"
                );
            }
            publish(root, &state)?;
            std::fs::remove_file(journal)?;
            state
        } else if root.join("config.json").exists() {
            read_state(root)?
        } else {
            ensure!(
                !root.join("providers.json").exists() && !root.join("models.json").exists(),
                "incomplete_config_directory"
            );
            let state = State {
                schema_version: 1,
                installation_id: format!("{:016x}{:016x}", crate::random_id(), crate::random_id()),
                revision: 0,
                owners: BTreeMap::new(),
            };
            atomic_json(&journal, &state)?;
            publish(root, &state)?;
            std::fs::remove_file(&journal)?;
            state
        };
        validate(&state)?;
        crate::builtin_skills::mirror(root)?;
        let mut retained = state
            .owners
            .values()
            .flat_map(|o| o.skills.values().map(|s| s.root.clone()))
            .collect::<std::collections::HashSet<_>>();
        if let Ok(previous) = read_json::<State>(&root.join("backups/config.previous.json")) {
            retained.extend(
                previous
                    .owners
                    .values()
                    .flat_map(|o| o.skills.values().map(|s| s.root.clone())),
            );
        }
        crate::extensions::collect_old_versions(root, &retained)?;
        Ok(Self {
            root: root.into(),
            state: Mutex::new(Arc::new(state)),
            catalog: crate::catalog::Catalog::new(root),
            uploads: Mutex::new(()),
        })
    }
    pub fn snapshot(&self, owner: &str) -> View {
        let s = self.state.lock().unwrap();
        View {
            schema_version: 1,
            installation_id: s.installation_id.clone(),
            revision: s.revision,
            config: s.owners.get(owner).cloned().unwrap_or_default(),
        }
    }
    pub fn execute(&self, owner: &str, command: Command) -> Result<serde_json::Value> {
        match command {
            Command::CredentialSet {
                name,
                value,
                expected_revision,
            } => {
                ai_terminal_agent_runtime::config::valid_id(&name)?;
                let mut current = self.state.lock().unwrap();
                ensure!(
                    current.revision == expected_revision,
                    "config_revision_conflict"
                );
                let reference = crate::secrets::put(&self.root, owner, &value)?;
                let mut candidate = (**current).clone();
                candidate
                    .owners
                    .entry(owner.into())
                    .or_default()
                    .credentials
                    .insert(name, reference.clone());
                candidate.revision = candidate
                    .revision
                    .checked_add(1)
                    .context("config_revision_exhausted")?;
                if let Err(error) = self.commit(&current, &candidate) {
                    if !self.root.join("config.pending.json").exists() {
                        let _ = crate::secrets::remove(&self.root, owner, &reference);
                    }
                    return Err(error);
                }
                *current = Arc::new(candidate);
                drop(current);
                Ok(serde_json::to_value(self.snapshot(owner))?)
            }
            Command::SkillEdit {
                id,
                path,
                body,
                expected_revision,
            } => {
                ai_terminal_agent_runtime::extensions::user_id(&id)?;
                let mut view = self.snapshot(owner);
                ensure!(
                    view.revision == expected_revision,
                    "config_revision_conflict"
                );
                let skill = view.config.skills.get(&id).context("skill_not_found")?;
                let mut files = crate::extensions::package_files(skill)?;
                ensure!(body.len() <= 512 * 1024, "skill_edit_limit");
                files.insert(path, body.into_bytes());
                let skill =
                    crate::extensions::install_files(&self.root, owner, &id, files, "edited")?;
                view.config.skills.insert(id, skill);
                Ok(serde_json::to_value(self.call(
                    owner,
                    Command::Replace {
                        expected_revision,
                        config: view.config,
                        secrets: BTreeMap::new(),
                    },
                )?)?)
            }
            Command::SkillUploadBegin {
                id,
                expected_revision,
            } => {
                let _guard = self.uploads.lock().unwrap();
                ensure!(
                    self.snapshot(owner).revision == expected_revision,
                    "config_revision_conflict"
                );
                crate::extensions::upload_begin(&self.root, owner, &id, expected_revision)
            }
            Command::SkillUploadChunk {
                upload_id,
                path,
                offset,
                data,
            } => {
                let _guard = self.uploads.lock().unwrap();
                crate::extensions::upload_chunk(&self.root, owner, &upload_id, &path, offset, &data)
            }
            Command::SkillUploadCommit { upload_id } => {
                let guard = self.uploads.lock().unwrap();
                let (id, path, expected_revision, directory) =
                    crate::extensions::upload_finish(&self.root, owner, &upload_id)?;
                let result = self.execute(
                    owner,
                    Command::SkillInstall {
                        id,
                        path,
                        expected_revision,
                    },
                );
                if result.is_ok() {
                    std::fs::remove_dir_all(directory)?;
                }
                drop(guard);
                result
            }
            Command::SkillInstall {
                id,
                path,
                expected_revision,
            } => {
                let mut view = self.snapshot(owner);
                ensure!(
                    view.revision == expected_revision,
                    "config_revision_conflict"
                );
                let id = id.strip_prefix("user/").unwrap_or(&id).to_owned();
                let skill = crate::extensions::install(&self.root, owner, &id, &path)?;
                view.config.skills.insert(id, skill);
                Ok(serde_json::to_value(self.call(
                    owner,
                    Command::Replace {
                        expected_revision,
                        config: view.config,
                        secrets: BTreeMap::new(),
                    },
                )?)?)
            }
            Command::SkillFiles {
                id,
                files,
                expected_revision,
            } => {
                use base64::Engine;
                let mut view = self.snapshot(owner);
                ensure!(
                    view.revision == expected_revision,
                    "config_revision_conflict"
                );
                let files = files
                    .into_iter()
                    .map(|(name, data)| {
                        Ok((
                            name,
                            base64::engine::general_purpose::STANDARD.decode(data)?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?;
                let skill =
                    crate::extensions::install_files(&self.root, owner, &id, files, "uploaded")?;
                view.config.skills.insert(id, skill);
                Ok(serde_json::to_value(self.call(
                    owner,
                    Command::Replace {
                        expected_revision,
                        config: view.config,
                        secrets: BTreeMap::new(),
                    },
                )?)?)
            }
            Command::SkillRead { id, path } => {
                let view = self.snapshot(owner);
                let skill = view
                    .config
                    .skills
                    .get(id.strip_prefix("user/").unwrap_or(&id))
                    .context("skill_not_found")?;
                let bytes = crate::extensions::read(skill, &path)?;
                ensure!(bytes.len() <= 512 * 1024, "skill_resource_limit");
                Ok(
                    serde_json::json!({"id":id,"path":path,"body":std::str::from_utf8(&bytes).context("binary_skill_resource")?,"version":skill.version}),
                )
            }
            Command::McpValidate { id } => {
                let view = self.snapshot(owner);
                let frozen = crate::extensions::Frozen::new(
                    &self.root,
                    owner,
                    view.revision,
                    Arc::new(view.config),
                    "",
                    None,
                )?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let result = runtime.block_on(frozen.tools(&id, None));
                frozen.close();
                Ok(result?.value)
            }
            Command::Discover {
                provider,
                search,
                cursor,
                refresh,
            } => {
                let view = self.snapshot(owner);
                let provider = view
                    .config
                    .providers
                    .get(&provider)
                    .context("provider_not_found")?;
                let secret = self.provider_secret(owner, provider)?;
                Ok(serde_json::to_value(self.catalog.discover(
                    owner,
                    view.revision,
                    provider,
                    &secret,
                    &search,
                    cursor.as_deref(),
                    refresh,
                )?)?)
            }
            command => Ok(serde_json::to_value(self.call(owner, command)?)?),
        }
    }
    pub fn call(&self, owner: &str, command: Command) -> Result<View> {
        match command {
            Command::SkillEdit { .. }
            | Command::SkillUploadBegin { .. }
            | Command::SkillUploadChunk { .. }
            | Command::SkillUploadCommit { .. }
            | Command::Discover { .. }
            | Command::CredentialSet { .. }
            | Command::SkillInstall { .. }
            | Command::SkillFiles { .. }
            | Command::SkillRead { .. }
            | Command::McpValidate { .. } => {
                anyhow::bail!("extended configuration requests use execute")
            }
            Command::Show => {}
            Command::Validate => {
                validate(&read_state(&self.root)?)?;
            }
            Command::Reload => {
                let mut current = self.state.lock().unwrap();
                let candidate = read_state(&self.root)?;
                validate(&candidate)?;
                ensure!(
                    candidate.installation_id == current.installation_id,
                    "installation_identity_changed"
                );
                ensure!(
                    candidate.revision == current.revision,
                    "config_revision_conflict"
                );
                let mut candidate = candidate;
                candidate.revision = candidate
                    .revision
                    .checked_add(1)
                    .context("config_revision_exhausted")?;
                self.commit(&current, &candidate)?;
                *current = Arc::new(candidate);
            }
            Command::Replace {
                expected_revision,
                mut config,
                secrets,
            } => {
                config.validate()?;
                let mut current = self.state.lock().unwrap();
                ensure!(
                    current.revision == expected_revision,
                    "config_revision_conflict"
                );
                ensure!(current.revision < u64::MAX, "config_revision_exhausted");
                ensure!(
                    secrets.len() <= config.providers.len(),
                    "invalid_secret_update"
                );
                for id in secrets.keys() {
                    ensure!(
                        config.providers.contains_key(id),
                        "secret_provider_not_found"
                    );
                    ensure!(
                        config.providers[id].credential_revision < u64::MAX,
                        "credential_revision_exhausted"
                    );
                }
                // Clients may retain a reference already owned by this account, never attach arbitrary vault identities.
                for (id, p) in &config.providers {
                    let old = current.owners.get(owner).and_then(|c| c.providers.get(id));
                    ensure!(
                        p.secret_ref.as_ref() == old.and_then(|p| p.secret_ref.as_ref())
                            || p.secret_ref.as_ref().is_some_and(|r| current
                                .owners
                                .get(owner)
                                .is_some_and(|o| o.credentials.values().any(|v| v == r))),
                        "secret_reference_is_read_only"
                    );
                    ensure!(
                        p.credential_revision == old.map_or(0, |p| p.credential_revision),
                        "credential_revision_is_read_only"
                    );
                }
                let previous = current.owners.get(owner).cloned().unwrap_or_default();
                ensure!(
                    config.credentials == previous.credentials,
                    "credential_references_are_read_only"
                );
                for (id, skill) in &config.skills {
                    let expected = self
                        .root
                        .join("skills")
                        .join(blake3::hash(owner.as_bytes()).to_hex().to_string())
                        .join(id.strip_prefix("user/").unwrap_or(id))
                        .join(&skill.version);
                    ensure!(skill.root == expected, "invalid_managed_skill_root");
                    let _ = crate::extensions::read(skill, "SKILL.md")?;
                }
                let mut written = Vec::new();
                let normalized = (|| -> Result<()> {
                    for (id, server) in &mut config.mcp {
                        server.transport = Some(server.wire()?.into());
                        for (kind, values, refs) in [
                            (
                                "env",
                                std::mem::take(&mut server.env),
                                &mut server.env_secret_refs,
                            ),
                            (
                                "header",
                                std::mem::take(&mut server.headers),
                                &mut server.header_secret_refs,
                            ),
                        ] {
                            for (name, value) in values {
                                let reference = crate::secrets::put(&self.root, owner, &value)?;
                                written.push(reference.clone());
                                let alias = format!(
                                    "mcp-{}",
                                    &blake3::hash(format!("{id}:{kind}:{name}").as_bytes())
                                        .to_hex()
                                        .as_str()[..24]
                                );
                                config.credentials.insert(alias.clone(), reference);
                                refs.insert(name, alias);
                            }
                        }
                        for alias in server
                            .env_secret_refs
                            .values()
                            .chain(server.header_secret_refs.values())
                        {
                            ensure!(
                                config.credentials.contains_key(alias),
                                "credential_reference_not_found"
                            );
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = normalized {
                    for reference in written {
                        let _ = crate::secrets::remove(&self.root, owner, &reference);
                    }
                    return Err(error);
                }
                for (id, secret) in secrets {
                    match crate::secrets::put(&self.root, owner, &secret) {
                        Ok(reference) => {
                            written.push(reference.clone());
                            let p = config.providers.get_mut(&id).unwrap();
                            p.secret_ref = Some(reference);
                            p.credential_revision += 1;
                            config
                                .credentials
                                .insert(format!("provider-{}", id), p.secret_ref.clone().unwrap());
                        }
                        Err(error) => {
                            for reference in written {
                                let _ = crate::secrets::remove(&self.root, owner, &reference);
                            }
                            return Err(error);
                        }
                    }
                }
                let mut candidate = (**current).clone();
                candidate.owners.insert(owner.into(), config);
                candidate.revision = candidate
                    .revision
                    .checked_add(1)
                    .context("config_revision_exhausted")?;
                if let Err(error) = self.commit(&current, &candidate) {
                    // A journal means this candidate can still recover: retain its secrets.
                    if !self.root.join("config.pending.json").exists() {
                        for reference in written {
                            let _ = crate::secrets::remove(&self.root, owner, &reference);
                        }
                    }
                    return Err(error);
                }
                *current = Arc::new(candidate);
            }
        }
        Ok(self.snapshot(owner))
    }
    pub fn authorization_secrets(&self, owner: &str) -> Result<Vec<String>> {
        let view = self.snapshot(owner);
        let mut secrets = Vec::new();
        for provider in view.config.providers.values() {
            let value = self.provider_secret(owner, provider)?;
            if !value.is_empty() {
                secrets.push(value);
            }
        }
        for reference in view.config.credentials.values() {
            let value = crate::secrets::get(&self.root, owner, reference)?;
            if !value.is_empty() {
                secrets.push(value);
            }
        }
        secrets.sort();
        secrets.dedup();
        Ok(secrets)
    }
    pub fn provider_secret(
        &self,
        owner: &str,
        provider: &ai_terminal_agent_runtime::config::Provider,
    ) -> Result<String> {
        match &provider.secret_ref {
            Some(reference) => crate::secrets::get(&self.root, owner, reference),
            None => Ok(String::new()),
        }
    }
    fn commit(&self, previous: &State, state: &State) -> Result<()> {
        validate(state)?;
        let backups = self.root.join("backups");
        crate::service::secure_dir(&backups)?;
        ensure!(
            !self.root.join("config.pending.json").exists(),
            "config_recovery_required"
        );
        atomic_json(&backups.join("config.previous.json"), previous)?;
        self.commit_candidate(state)
    }
    fn commit_candidate(&self, state: &State) -> Result<()> {
        atomic_json(&self.root.join("config.pending.json"), state)?;
        publish(&self.root, state)?;
        std::fs::remove_file(self.root.join("config.pending.json"))?;
        sync_dir(&self.root)?;
        Ok(())
    }
}
fn validate(state: &State) -> Result<()> {
    ensure!(
        state.schema_version == 1 && !state.installation_id.is_empty(),
        "unsupported_config_schema"
    );
    ensure!(state.owners.len() <= 128, "owner_limit");
    for owner in state.owners.values() {
        owner.validate()?;
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Root {
    schema_version: u32,
    installation_id: String,
    revision: u64,
}
#[derive(Serialize, Deserialize)]
struct Providers {
    schema_version: u32,
    revision: u64,
    owners: BTreeMap<String, BTreeMap<String, ai_terminal_agent_runtime::config::Provider>>,
}
#[derive(Serialize, Deserialize)]
struct Models {
    schema_version: u32,
    revision: u64,
    owners: BTreeMap<String, ModelScope>,
}
#[derive(Serialize, Deserialize)]
struct ModelScope {
    #[serde(default)]
    terminal_reading: ai_terminal_agent_runtime::config::TerminalReading,
    #[serde(default)]
    skills: BTreeMap<String, ai_terminal_agent_runtime::extensions::Skill>,
    #[serde(default)]
    skill_sources: Vec<ai_terminal_agent_runtime::extensions::SkillSource>,
    #[serde(default)]
    credentials: BTreeMap<String, String>,
    models: BTreeMap<String, ai_terminal_agent_runtime::config::ModelProfile>,
    bindings: BTreeMap<String, ai_terminal_agent_runtime::config::Binding>,
}
#[derive(Serialize, Deserialize)]
struct McpFile {
    schema_version: u32,
    revision: u64,
    #[serde(rename = "mcpServers")]
    servers: BTreeMap<String, McpEntry>,
}
#[derive(Serialize, Deserialize)]
struct McpEntry {
    owner: String,
    id: String,
    #[serde(flatten)]
    config: ai_terminal_agent_runtime::extensions::McpServer,
}
fn publish(root: &Path, state: &State) -> Result<()> {
    let providers = Providers {
        schema_version: 1,
        revision: state.revision,
        owners: state
            .owners
            .iter()
            .map(|(k, v)| (k.clone(), v.providers.clone()))
            .collect(),
    };
    let models = Models {
        schema_version: 1,
        revision: state.revision,
        owners: state
            .owners
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    ModelScope {
                        terminal_reading: v.terminal_reading.clone(),
                        skills: v.skills.clone(),
                        skill_sources: v.skill_sources.clone(),
                        credentials: v.credentials.clone(),
                        models: v.models.clone(),
                        bindings: v.bindings.clone(),
                    },
                )
            })
            .collect(),
    };
    atomic_json(&root.join("providers.json"), &providers)?;
    atomic_json(&root.join("models.json"), &models)?;
    let mcp = McpFile {
        schema_version: 1,
        revision: state.revision,
        servers: state
            .owners
            .iter()
            .flat_map(|(owner, config)| {
                config.mcp.iter().map(move |(id, server)| {
                    (
                        format!(
                            "{}/{}",
                            &blake3::hash(owner.as_bytes()).to_hex().as_str()[..16],
                            id
                        ),
                        McpEntry {
                            owner: owner.clone(),
                            id: id.clone(),
                            config: server.clone(),
                        },
                    )
                })
            })
            .collect(),
    };
    atomic_json(&root.join("mcp.json"), &mcp)?;
    atomic_json(
        &root.join("config.json"),
        &Root {
            schema_version: 1,
            installation_id: state.installation_id.clone(),
            revision: state.revision,
        },
    )?;
    Ok(())
}
fn read_state(root: &Path) -> Result<State> {
    let meta: Root = read_json(&root.join("config.json"))?;
    let providers: Providers = read_json(&root.join("providers.json"))?;
    let models: Models = read_json(&root.join("models.json"))?;
    ensure!(
        meta.schema_version == 1 && providers.schema_version == 1 && models.schema_version == 1,
        "unsupported_config_schema"
    );
    ensure!(
        meta.revision == providers.revision && meta.revision == models.revision,
        "incomplete_config_transaction"
    );
    let mut owners = BTreeMap::new();
    for (id, providers) in providers.owners {
        owners
            .entry(id)
            .or_insert_with(OwnerConfig::default)
            .providers = providers;
    }
    for (id, models) in models.owners {
        let owner = owners.entry(id).or_insert_with(OwnerConfig::default);
        owner.terminal_reading = models.terminal_reading;
        owner.models = models.models;
        owner.bindings = models.bindings;
        owner.skills = models.skills;
        owner.skill_sources = models.skill_sources;
        owner.credentials = models.credentials;
    }
    if root.join("mcp.json").exists() {
        let mcp: McpFile = read_json(&root.join("mcp.json"))?;
        ensure!(
            mcp.schema_version == 1 && mcp.revision == meta.revision,
            "incomplete_config_transaction"
        );
        for entry in mcp.servers.into_values() {
            owners
                .entry(entry.owner)
                .or_insert_with(OwnerConfig::default)
                .mcp
                .insert(entry.id, entry.config);
        }
    }
    Ok(State {
        schema_version: 1,
        installation_id: meta.installation_id,
        revision: meta.revision,
        owners,
    })
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 4 * 1024 * 1024,
        "invalid_config_file"
    );
    serde_json::from_slice(&std::fs::read(path)?).context("invalid config JSON")
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    ensure!(bytes.len() <= 4 * 1024 * 1024, "config_limit");
    let tmp = path.with_extension("tmp");
    let mut file = crate::service::open_private(&tmp, false)?;
    file.set_len(0)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(tmp, path)?;
    sync_dir(path.parent().unwrap())?;
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_preserves_unmanaged_partial_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        crate::service::secure_dir(&root).unwrap();
        std::fs::write(root.join("providers.json"), b"existing user configuration").unwrap();
        assert!(ConfigService::open(&root).is_err());
        assert_eq!(
            std::fs::read(root.join("providers.json")).unwrap(),
            b"existing user configuration"
        );
    }
    #[test]
    fn config_revision_and_recovery_are_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        let service = ConfigService::open(&root).unwrap();
        let old = service.snapshot("owner");
        assert_eq!(old.revision, 0);
        service
            .call(
                "owner",
                Command::Replace {
                    expected_revision: 0,
                    config: OwnerConfig::default(),
                    secrets: BTreeMap::new(),
                },
            )
            .unwrap();
        assert!(
            service
                .call(
                    "owner",
                    Command::Replace {
                        expected_revision: 0,
                        config: OwnerConfig::default(),
                        secrets: BTreeMap::new(),
                    }
                )
                .is_err()
        );
        let mut candidate = read_state(&root).unwrap();
        candidate.revision += 1;
        atomic_json(&root.join("config.pending.json"), &candidate).unwrap();
        std::fs::write(root.join("models.json"), b"interrupted").unwrap();
        drop(service);
        let restored = ConfigService::open(&root).unwrap();
        assert_eq!(restored.snapshot("owner").revision, 2);
        assert_eq!(restored.snapshot("other").config.providers.len(), 0);
    }
}

#[cfg(test)]
mod reading_settings_contracts {
    use super::*;
    #[test]
    fn defaults_and_custom_counts_survive_publication_without_mutating_old_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let service = ConfigService::open(&root).unwrap();
        let frozen = service.snapshot("owner").config;
        assert_eq!(frozen.terminal_reading.head_lines, 10);
        assert_eq!(frozen.terminal_reading.tail_lines, 20);
        let mut changed = frozen.clone();
        changed.terminal_reading.head_lines = 7;
        changed.terminal_reading.tail_lines = 31;
        service
            .call(
                "owner",
                Command::Replace {
                    expected_revision: 0,
                    config: changed,
                    secrets: Default::default(),
                },
            )
            .unwrap();
        assert_eq!(frozen.terminal_reading.tail_lines, 20);
        let reopened = ConfigService::open(&root).unwrap().snapshot("owner");
        assert_eq!(reopened.config.terminal_reading.head_lines, 7);
        assert_eq!(reopened.config.terminal_reading.tail_lines, 31);
        let mut invalid = reopened.config;
        invalid.terminal_reading.tail_lines = 0;
        assert!(invalid.validate().is_err());
        invalid.terminal_reading.tail_lines = 101;
        assert!(invalid.validate().is_err());
    }
}
