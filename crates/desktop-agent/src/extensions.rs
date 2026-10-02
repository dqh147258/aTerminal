//! Managed immutable skill versions and per-run extension bindings.
use ai_terminal_agent_runtime::{
    config::OwnerConfig,
    extensions::{McpServer, Skill},
    host::{Observation, ToolContext, ToolOutput},
    mcp::{CatalogSnapshot, Connection, TransportConfig},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::AsyncReadExt;
#[derive(Deserialize)]
struct Front {
    name: String,
    description: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    files: BTreeMap<String, String>,
}
fn owner_key(owner: &str) -> String {
    blake3::hash(owner.as_bytes()).to_hex().to_string()
}
fn relative(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "invalid_skill_resource_path"
    );
    Ok(path)
}
fn collect(
    root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    depth: usize,
) -> Result<()> {
    ensure!(depth <= 8 && files.len() < 256, "skill_package_limit");
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let canonical = path.canonicalize()?;
        ensure!(canonical.starts_with(root), "skill_symlink_escapes_package");
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(
            !meta.file_type().is_symlink() || !canonical.is_dir(),
            "nested_skill_directory_symlink_not_supported"
        );
        if canonical.is_dir() {
            collect(root, &canonical, files, depth + 1)?;
        } else {
            ensure!(meta.len() <= 2 * 1024 * 1024, "skill_file_limit");
            let key = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            relative(&key)?;
            files.insert(key, std::fs::read(canonical)?);
            ensure!(
                files.values().map(Vec::len).sum::<usize>() <= 8 * 1024 * 1024,
                "skill_package_limit"
            );
        }
    }
    Ok(())
}
pub fn install(root: &Path, owner: &str, id: &str, source: &Path) -> Result<Skill> {
    ai_terminal_agent_runtime::extensions::user_id(id)?;
    let source = source.canonicalize()?;
    let mut files = BTreeMap::new();
    collect(&source, &source, &mut files, 0)?;
    install_files(root, owner, id, files, "installed")
}
pub fn install_files(
    root: &Path,
    owner: &str,
    id: &str,
    files: BTreeMap<String, Vec<u8>>,
    source: &str,
) -> Result<Skill> {
    ai_terminal_agent_runtime::extensions::user_id(id)?;
    ensure!(
        files.len() <= 256
            && files.values().all(|v| v.len() <= 2 * 1024 * 1024)
            && files.values().map(Vec::len).sum::<usize>() <= 8 * 1024 * 1024,
        "skill_package_limit"
    );
    let body = std::str::from_utf8(files.get("SKILL.md").context("skill_md_required")?)?;
    ensure!(
        body.starts_with("---\n") || body.starts_with("---\r\n"),
        "skill_frontmatter_required"
    );
    let body = body.replace("\r\n", "\n");
    let end = body[4..]
        .find("\n---")
        .context("skill_frontmatter_unclosed")?
        + 4;
    ensure!(end <= 16384, "skill_metadata_limit");
    let metadata: Front =
        serde_yaml::from_str(&body[4..end]).context("invalid_skill_frontmatter")?;
    ensure!(
        !metadata.name.trim().is_empty()
            && metadata.name.len() <= 128
            && !metadata.description.trim().is_empty()
            && metadata.description.len() <= 4096,
        "invalid_skill_metadata"
    );
    let policy = files
        .get("agents/openai.yaml")
        .map(|data| serde_yaml::from_slice::<serde_yaml::Value>(data))
        .transpose()?
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or(Value::Null);
    ai_terminal_agent_runtime::config::valid_id(&metadata.name)?;
    let allow_implicit = policy
        .pointer("/policy/allow_implicit_invocation")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let dependencies = policy
        .pointer("/dependencies/tools")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|d| d["type"] == "mcp")
                .filter_map(|d| d["value"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let manifest = Manifest {
        files: files
            .iter()
            .map(|(name, data)| (name.clone(), blake3::hash(data).to_hex().to_string()))
            .collect(),
    };
    let mut hash = blake3::Hasher::new();
    for name in files.keys() {
        relative(name)?;
    }
    hash.update(&serde_json::to_vec(&manifest.files)?);
    let version = hash.finalize().to_hex().to_string();
    let directory = root
        .join("skills")
        .join(owner_key(owner))
        .join(id.strip_prefix("user/").unwrap_or(id))
        .join(&version);
    if !directory.exists() {
        let staging = directory.with_extension(format!("staging-{:016x}", crate::random_id()));
        crate::service::secure_dir(&staging)?;
        for (name, data) in &files {
            let path = staging.join(relative(name)?);
            crate::service::secure_dir(path.parent().unwrap())?;
            let mut file = crate::service::open_private(&path, false)?;
            file.set_len(0)?;
            file.write_all(data)?;
            file.sync_all()?;
        }
        let manifest = Manifest {
            files: files
                .iter()
                .map(|(n, b)| (n.clone(), blake3::hash(b).to_hex().to_string()))
                .collect(),
        };
        let mut file =
            crate::service::open_private(&directory.with_extension("manifest.json"), false)?;
        serde_json::to_writer(&mut file, &manifest)?;
        file.sync_all()?;
        std::fs::rename(&staging, &directory)?;
        #[cfg(unix)]
        std::fs::File::open(directory.parent().unwrap())?.sync_all()?;
    }
    Ok(Skill {
        id: id.into(),
        name: metadata.name,
        description: metadata.description,
        version,
        root: directory,
        enabled: true,
        allow_implicit,
        dependencies,
        source: source.into(),
        interface: policy["interface"].clone(),
    })
}
pub fn read(skill: &Skill, path: &str) -> Result<Vec<u8>> {
    let file = relative(path)?;
    let root = skill.root.canonicalize()?;
    let target = root.join(&file);
    ensure!(
        target.canonicalize()?.starts_with(&root),
        "skill_resource_escape"
    );
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(root.with_extension("manifest.json"))?)?;
    ensure!(
        blake3::hash(&serde_json::to_vec(&manifest.files)?)
            .to_hex()
            .as_str()
            == skill.version,
        "skill_manifest_changed"
    );
    let expected = manifest
        .files
        .get(path)
        .context("skill_resource_not_found")?;
    let data = std::fs::read(target)?;
    ensure!(
        data.len() <= 2 * 1024 * 1024 && blake3::hash(&data).to_hex().as_str() == expected,
        "skill_version_changed"
    );
    Ok(data)
}
pub fn minimal_environment() -> BTreeMap<String, String> {
    [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "TEMP",
        "TMP",
        "LANG",
    ]
    .into_iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| (k.into(), v)))
    .collect()
}
fn program(command: &Path, env: &BTreeMap<String, String>) -> Result<PathBuf> {
    if command.is_absolute() {
        ensure!(command.is_file(), "extension_executable_not_found");
        return Ok(command.into());
    }
    ensure!(
        command.components().count() == 1,
        "extension_command_must_be_name_or_absolute_path"
    );
    for directory in std::env::split_paths(&env.get("PATH").cloned().unwrap_or_default()) {
        let path = directory.join(command);
        if path.is_file() {
            return Ok(path);
        }
        #[cfg(windows)]
        for suffix in ["exe", "cmd", "bat"] {
            let path = path.with_extension(suffix);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    bail!("extension_executable_not_found")
}
struct ServerBinding {
    config: McpServer,
    catalog: tokio::sync::OnceCell<Arc<CatalogSnapshot>>,
}
pub struct Frozen {
    root: PathBuf,
    owner: String,
    revision: u64,
    config: Arc<OwnerConfig>,
    servers: BTreeMap<String, ServerBinding>,
    skills: BTreeMap<String, Skill>,
    explicit: Arc<Mutex<Vec<String>>>,
    devices: Arc<Mutex<std::collections::BTreeSet<String>>>,
    cwd: Option<PathBuf>,
}
impl Frozen {
    pub fn new(
        root: &Path,
        owner: &str,
        revision: u64,
        config: Arc<OwnerConfig>,
        user_message: &str,
        cwd: Option<PathBuf>,
    ) -> Result<Arc<Self>> {
        let mut skills = config
            .skills
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(id, s)| {
                (
                    format!("user/{}", id.strip_prefix("user/").unwrap_or(id)),
                    s.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for skill in skills.values_mut() {
            let body = read(skill, "SKILL.md")?;
            let text = std::str::from_utf8(&body)?.replace("\r\n", "\n");
            ensure!(text.starts_with("---\n"), "skill_frontmatter_required");
            let end = text[4..]
                .find("\n---")
                .context("skill_frontmatter_unclosed")?
                + 4;
            let front: Front = serde_yaml::from_str(&text[4..end])?;
            skill.name = front.name;
            skill.description = front.description;
            let manifest: Manifest = serde_json::from_slice(&std::fs::read(
                skill.root.with_extension("manifest.json"),
            )?)?;
            let policy = if manifest.files.contains_key("agents/openai.yaml") {
                serde_json::to_value(serde_yaml::from_slice::<serde_yaml::Value>(&read(
                    skill,
                    "agents/openai.yaml",
                )?)?)?
            } else {
                Value::Null
            };
            skill.allow_implicit = policy
                .pointer("/policy/allow_implicit_invocation")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            skill.dependencies = policy
                .pointer("/dependencies/tools")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter(|d| d["type"] == "mcp")
                        .filter_map(|d| d["value"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            skill.interface = policy["interface"].clone();
        }
        for source in config.skill_sources.iter().filter(|s| s.enabled) {
            let roots = if source.project {
                let Some(mut directory) = cwd.clone() else {
                    continue;
                };
                let mut roots = Vec::new();
                let mut repository = false;
                for _ in 0..32 {
                    roots.push(directory.join(".agents/skills"));
                    if directory.join(".git").exists() {
                        repository = true;
                        break;
                    }
                    if !directory.pop() {
                        break;
                    }
                }
                if !repository {
                    roots.truncate(1);
                }
                roots
            } else {
                vec![source.path.clone().context("skill_source_path_required")?]
            };
            for directory in roots {
                if !directory.is_dir() {
                    continue;
                }
                for entry in std::fs::read_dir(directory)?.take(128) {
                    let path = entry?.path();
                    if !path.join("SKILL.md").is_file() {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .context("skill_name_required")?
                        .to_string_lossy()
                        .to_string();
                    let id = format!("{}-{}", source.id, name);
                    if let Ok(skill) = install(root, owner, &id, &path) {
                        skills.insert(format!("source/{}/{}", source.id, name), skill);
                    }
                }
            }
        }
        ensure!(skills.len() <= 256, "skill_catalog_limit");
        let servers = config
            .mcp
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(id, s)| {
                (
                    id.clone(),
                    ServerBinding {
                        config: s.clone(),
                        catalog: tokio::sync::OnceCell::new(),
                    },
                )
            })
            .collect();
        Ok(Arc::new(Self {
            root: root.into(),
            owner: owner.into(),
            revision,
            config,
            servers,
            skills,
            explicit: Arc::new(Mutex::new(vec![user_message.into()])),
            devices: Arc::new(Mutex::new(Default::default())),
            cwd,
        }))
    }
    pub fn for_session(&self, cwd: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            root: self.root.clone(),
            owner: self.owner.clone(),
            revision: self.revision,
            config: self.config.clone(),
            skills: self.skills.clone(),
            explicit: self.explicit.clone(),
            devices: self.devices.clone(),
            cwd,
            servers: self
                .servers
                .iter()
                .map(|(id, s)| {
                    (
                        id.clone(),
                        ServerBinding {
                            config: s.config.clone(),
                            catalog: tokio::sync::OnceCell::new(),
                        },
                    )
                })
                .collect(),
        })
    }
    pub fn check_credentials(&self, current: &OwnerConfig) -> Result<()> {
        for (id, server) in &self.servers {
            let active = current
                .mcp
                .get(id)
                .filter(|s| s.enabled)
                .context("extension_removed_or_disabled")?;
            ensure!(
                serde_json::to_value(active)? == serde_json::to_value(&server.config)?,
                "extension_version_changed"
            );
            for alias in server
                .config
                .env_secret_refs
                .values()
                .chain(server.config.header_secret_refs.values())
            {
                ensure!(
                    self.config.credentials.get(alias) == current.credentials.get(alias),
                    "extension_credentials_revoked"
                );
            }
        }
        for (id, skill) in &self.config.skills {
            if skill.enabled {
                let active = current
                    .skills
                    .get(id)
                    .filter(|s| s.enabled)
                    .context("skill_removed_or_disabled")?;
                ensure!(
                    active.version == skill.version && active.root == skill.root,
                    "skill_version_changed"
                );
            }
        }
        ensure!(
            serde_json::to_value(&self.config.skill_sources)?
                == serde_json::to_value(&current.skill_sources)?,
            "skill_sources_changed"
        );
        Ok(())
    }
    pub fn server_ids(&self) -> Value {
        json!(self.servers.keys().collect::<Vec<_>>())
    }
    pub fn add_device(&self, device: &str) {
        self.devices.lock().unwrap().insert(device.into());
    }
    pub fn devices(&self) -> Vec<String> {
        self.devices.lock().unwrap().iter().cloned().collect()
    }
    pub fn user_message(&self, message: &str) {
        let mut messages = self.explicit.lock().unwrap();
        if messages.len() < 32 {
            messages.push(message.into());
        }
    }
    pub fn metadata(&self, query: &str, cursor: Option<&str>) -> Result<Value> {
        let offset = cursor.map(str::parse::<usize>).transpose()?.unwrap_or(0);
        let items=self.skills.iter().filter(|(id,s)|id.contains(query)||s.description.contains(query)||s.name.contains(query)).map(|(id,s)|json!({"id":id,"name":s.name,"description":s.description,"version":s.version,"source":s.source,"allow_implicit":s.allow_implicit,"dependencies":s.dependencies})).collect::<Vec<_>>();
        ensure!(offset <= items.len(), "invalid_skill_cursor");
        let end = (offset + 20).min(items.len());
        Ok(
            json!({"skills":items[offset..end],"cursor":if end<items.len(){Some(end.to_string())}else{None}}),
        )
    }
    fn selected(&self, id: &str) -> Result<&Skill> {
        let skill = self.skills.get(id).context("skill_not_found")?;
        if !skill.allow_implicit {
            let messages = self.explicit.lock().unwrap();
            let names = self
                .skills
                .values()
                .filter(|s| s.name == skill.name)
                .count();
            ensure!(
                messages.iter().any(|m| explicit_invocation(m, id)
                    || (names == 1 && explicit_invocation(m, &skill.name))),
                "skill_requires_explicit_user_invocation"
            );
        }
        for dependency in &skill.dependencies {
            ensure!(
                dependency == "builtin/terminal" || self.servers.contains_key(dependency),
                "skill_mcp_dependency_missing"
            );
        }
        Ok(skill)
    }
    pub fn resource(
        &self,
        id: &str,
        path: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<Value> {
        let skill = self.selected(id)?;
        let bytes = read(skill, path)?;
        let body = std::str::from_utf8(&bytes).context("binary_skill_resource")?;
        let start = cursor.map(str::parse::<usize>).transpose()?.unwrap_or(0);
        ensure!(
            start <= body.len() && body.is_char_boundary(start),
            "invalid_skill_cursor"
        );
        let mut end = (start + limit.clamp(4, 16384)).min(body.len());
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        Ok(
            json!({"skill_id":id,"version":skill.version,"path":path,"body":&body[start..end],"cursor":if end<body.len(){Some(end.to_string())}else{None}}),
        )
    }
    fn secret(&self, alias: &str) -> Result<String> {
        let reference = self
            .config
            .credentials
            .get(alias)
            .context("credential_reference_not_found")?;
        crate::secrets::get(&self.root, &self.owner, reference)
    }
    async fn server(&self, id: &str) -> Result<Arc<CatalogSnapshot>> {
        let binding = self.servers.get(id).context("mcp_server_not_selected")?;
        binding
            .catalog
            .get_or_try_init(|| async {
                let server = &binding.config;
                let mut env = minimal_environment();
                env.extend(server.env.clone());
                for (name, alias) in &server.env_secret_refs {
                    env.insert(name.clone(), self.secret(alias)?);
                }
                let transport = if server.wire()? == "stdio" {
                    TransportConfig::Stdio {
                        command: program(
                            server.command.as_ref().context("mcp_command_required")?,
                            &env,
                        )?,
                        args: server.args.clone(),
                        cwd: server.cwd.clone().or_else(|| self.cwd.clone()),
                        env,
                    }
                } else {
                    let mut headers = BTreeMap::new();
                    for (name, alias) in &server.header_secret_refs {
                        headers.insert(name.clone(), self.secret(alias)?);
                    }
                    TransportConfig::StreamableHttp {
                        url: server.url.clone().context("mcp_url_required")?,
                        headers,
                    }
                };
                let connection = Arc::new(
                    Connection::connect(
                        &transport,
                        Duration::from_millis(server.startup_timeout_ms),
                    )
                    .await?,
                );
                Ok(Arc::new(
                    connection
                        .catalog(
                            id.into(),
                            self.revision,
                            Duration::from_millis(server.startup_timeout_ms),
                        )
                        .await?,
                ))
            })
            .await
            .cloned()
    }
    pub async fn tools(&self, id: &str, cursor: Option<&str>) -> Result<ToolOutput> {
        let catalog = self.server(id).await?;
        let start = cursor.map(str::parse::<usize>).transpose()?.unwrap_or(0);
        ensure!(start <= catalog.tools.len(), "invalid_mcp_cursor");
        let end = (start + 20).min(catalog.tools.len());
        Ok(ToolOutput::value(
            json!({"source":"user_mcp","server_id":id,"revision":catalog.revision,"diagnostics":catalog.connection.diagnostics(),"tools":catalog.tools[start..end],"cursor":if end<catalog.tools.len(){Some(end.to_string())}else{None}}),
        ))
    }
    pub fn authorization_descriptor(
        &self,
        descriptor: &mut ai_terminal_agent_runtime::authorization::ActionDescriptor,
    ) -> Result<()> {
        use ai_terminal_agent_runtime::authorization::ToolSource;
        if descriptor.tool == "mcp_call" {
            descriptor.source = ToolSource::Mcp;
            let id = descriptor.arguments["server_id"]
                .as_str()
                .context("server_id_required")?;
            let binding = self.servers.get(id).context("mcp_server_not_found")?;
            descriptor.source_id = id.into();
            descriptor.tool_version = Some(
                blake3::hash(&serde_json::to_vec(&binding.config)?)
                    .to_hex()
                    .to_string(),
            );
            descriptor.cwd = binding
                .config
                .cwd
                .as_ref()
                .or(self.cwd.as_ref())
                .map(|p| p.to_string_lossy().into_owned());
            // Remote service versions and interpreter dependencies cannot be pinned by
            // configuration alone. Such calls remain once/full only.
            descriptor.execution_identity = None;
        } else {
            descriptor.source = ToolSource::Skill;
            let id = descriptor.arguments["skill_id"]
                .as_str()
                .context("skill_id_required")?;
            let skill = self.selected(id)?;
            descriptor.source_id = id.into();
            descriptor.tool_version = Some(skill.version.clone());
            let args = &descriptor.arguments["arguments"];
            let path = args["path"].as_str().context("script_path_required")?;
            let bytes = read(skill, path)?;
            let executable = program(
                Path::new(
                    args["interpreter"]
                        .as_str()
                        .context("explicit_interpreter_required")?,
                ),
                &minimal_environment(),
            )?;
            let mut hasher = blake3::Hasher::new();
            hasher.update(&bytes);
            hasher.update(&std::fs::read(executable)?);
            descriptor.execution_identity = Some(hasher.finalize().to_hex().to_string());
            descriptor.cwd = match args["cwd"].as_str().unwrap_or("session") {
                "package" => Some(skill.root.to_string_lossy().into_owned()),
                "session" => self.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()),
                _ => bail!("invalid_script_cwd"),
            };
        }
        Ok(())
    }
    pub async fn call(
        &self,
        context: &ToolContext,
        id: &str,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput> {
        let catalog = self.server(id).await?;
        let definition = catalog
            .tools
            .iter()
            .find(|t| t.name == tool)
            .context("mcp_tool_not_found")?;
        let schema = Value::Object((*definition.input_schema).clone());
        let validator = jsonschema::validator_for(&schema)?;
        ensure!(validator.is_valid(&args), "invalid_mcp_arguments");
        let timeout = context.budget.remaining()?.min(Duration::from_millis(
            self.servers[id].config.call_timeout_ms,
        ));
        context.check_authorization()?;
        {
            // Commit is the action-start linearization point shared with user permission
            // changes and credential/identity cancellation. Never hold this std gate
            // across asynchronous transport I/O or recursively acquire it in preflight.
            let permitted = context.execution_gate.lock().unwrap();
            ensure!(
                *permitted && !*context.cancel.borrow(),
                "cancelled_before_mcp_call"
            );
            context.commit_authorization(None)?;
        }
        let result = catalog.connection.call(tool, args, timeout).await?;
        let body = serde_json::to_string(&result)?;
        if body.len() <= context.max_read_bytes
            && !result.content.iter().any(|c| {
                serde_json::to_value(c)
                    .ok()
                    .is_some_and(|v| v["type"] == "image")
            })
        {
            return Ok(ToolOutput::value(
                json!({"source":"user_mcp","server_id":id,"result":result}),
            ));
        }
        let preview = body
            .chars()
            .take(context.max_read_bytes / 4)
            .collect::<String>();
        Ok(ToolOutput {
            value: json!({"source":"user_mcp","server_id":id,"partial":true}),
            observation: Some(Observation {
                kind: "mcp_result".into(),
                metadata: json!({"source":"user_mcp","server_id":id}),
                body,
                model_body: Some(preview),
                binary: false,
                record_id: None,
            }),
            outcome: Some("accepted".into()),
        })
    }
    pub async fn script(&self, context: &ToolContext, id: &str, args: Value) -> Result<ToolOutput> {
        use process_wrap::tokio::*;
        let skill = self.selected(id)?;
        let path = args["path"].as_str().context("script_path_required")?;
        ensure!(path.starts_with("scripts/"), "skill_script_path_required");
        let _ = read(skill, path)?;
        let interpreter = args["interpreter"]
            .as_str()
            .context("explicit_interpreter_required")?;
        ensure!(
            [
                "python3",
                "python",
                "node",
                "bash",
                "sh",
                "pwsh",
                "powershell"
            ]
            .contains(&interpreter),
            "unsupported_script_interpreter"
        );
        let argv: Vec<String> = args
            .get("args")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        ensure!(
            argv.len() <= 64 && argv.iter().map(String::len).sum::<usize>() <= 16000,
            "script_argument_limit"
        );
        let env = minimal_environment();
        let executable = program(Path::new(interpreter), &env)?;
        let cwd = match args["cwd"].as_str().unwrap_or("session") {
            "package" => skill.root.clone(),
            "session" => self
                .cwd
                .clone()
                .context("current_session_cwd_unavailable; choose package cwd explicitly")?,
            _ => bail!("invalid_script_cwd"),
        };
        let mut command = CommandWrap::with_new(executable, |command| {
            command
                .arg(skill.root.join(path))
                .args(argv)
                .current_dir(cwd)
                .env_clear()
                .envs(env)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
        });
        #[cfg(unix)]
        command.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(JobObject);
        command.wrap(KillOnDrop);
        context.check_authorization()?;
        let mut child = {
            let permitted = context.execution_gate.lock().unwrap();
            ensure!(
                *permitted && !*context.cancel.borrow(),
                "cancelled_before_script_spawn"
            );
            context.commit_authorization(None)?;
            command.spawn()?
        };
        let stdout = child.stdout().take().context("script_stdout_missing")?;
        let stderr = child.stderr().take().context("script_stderr_missing")?;
        let mut out = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout
                .take(256 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        let mut err = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr
                .take(32769)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        let mut cancel = context.cancel.clone();
        let deadline = context.budget.remaining()?.min(Duration::from_secs(60));
        let completed = tokio::select! {
            biased;
            _=cancel.wait_for(|v|*v)=>Err(anyhow::anyhow!("script_cancelled_outcome_unknown")),
            result=tokio::time::timeout(deadline,async {
                let result=child.wait().await?;
                let stdout=(&mut out).await??;
                let stderr=(&mut err).await??;
                Ok::<_,anyhow::Error>((result,stdout,stderr))
            })=>result.unwrap_or_else(|_|Err(anyhow::anyhow!("script_timeout_outcome_unknown")))
        };
        if completed.is_err() {
            let _ = child.start_kill();
            out.abort();
            err.abort();
        }
        let (result, stdout, stderr) = completed?;
        ensure!(
            stdout.len() <= 256 * 1024 && stderr.len() <= 32768,
            "script_output_limit"
        );
        let body = String::from_utf8(stdout).context("script_output_not_utf8")?;
        let stderr = String::from_utf8_lossy(&stderr).to_string();
        let preview = body
            .chars()
            .take(context.max_read_bytes / 4)
            .collect::<String>();
        Ok(ToolOutput {
            value: json!({"exit_code":result.code(),"stderr":stderr,"skill_id":id}),
            observation: Some(Observation {
                kind: "script_output".into(),
                metadata: json!({"source":"user_skill","skill_id":id,"version":skill.version}),
                body,
                model_body: Some(preview),
                binary: false,
                record_id: None,
            }),
            outcome: Some("written".into()),
        })
    }
    pub fn close(&self) {
        for server in self.servers.values() {
            if let Some(catalog) = server.catalog.get() {
                catalog.connection.close();
            }
        }
    }
}

fn explicit_invocation(message: &str, name: &str) -> bool {
    message
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '$' | '/' | '_' | '-')))
        .any(|word| word.strip_prefix('$') == Some(name))
}

pub fn builtin_catalog() -> Vec<Value> {
    crate::builtin_skills::CATALOG.iter().map(|(id,description,body)|json!({"id":id,"description":description,"body":body,"builtin":true,"read_only":true})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_removed_or_reconfigured_extensions_revoke_frozen_calls() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = OwnerConfig::default();
        let server: McpServer = serde_json::from_value(
            json!({"command":"/usr/bin/printf","args":["fixed"],"enabled":true}),
        )
        .unwrap();
        config.mcp.insert("server".into(), server);
        let frozen = Frozen::new(
            temp.path(),
            "owner",
            1,
            Arc::new(config.clone()),
            "",
            Some(temp.path().into()),
        )
        .unwrap();
        frozen.check_credentials(&config).unwrap();
        let mut changed = config.clone();
        changed
            .mcp
            .get_mut("server")
            .unwrap()
            .args
            .push("new".into());
        assert!(frozen.check_credentials(&changed).is_err());
        let mut changed = config.clone();
        changed.mcp.get_mut("server").unwrap().enabled = false;
        assert!(frozen.check_credentials(&changed).is_err());
        let mut changed = config;
        changed.mcp.clear();
        assert!(frozen.check_credentials(&changed).is_err());
    }
    #[test]
    fn immutable_packages_enforce_policy_manifest_and_resource_boundaries() {
        let temp = tempfile::tempdir().unwrap();
        let files = BTreeMap::from([
            (
                "SKILL.md".into(),
                b"---\nname: example\ndescription: Test package\n---\nDetails".to_vec(),
            ),
            (
                "agents/openai.yaml".into(),
                b"policy:\n  allow_implicit_invocation: false\n".to_vec(),
            ),
            ("references/large.txt".into(), "中".repeat(100).into_bytes()),
        ]);
        let skill = install_files(temp.path(), "owner", "example", files, "test").unwrap();
        let mut config = OwnerConfig::default();
        config.skills.insert("example".into(), skill.clone());
        let frozen = Frozen::new(
            temp.path(),
            "owner",
            1,
            Arc::new(config),
            "$example-other",
            None,
        )
        .unwrap();
        assert!(
            frozen
                .resource("user/example", "SKILL.md", 100, None)
                .is_err()
        );
        frozen.user_message("Please run $example.");
        // Punctuation terminates an explicit invocation.
        assert!(
            frozen
                .resource("user/example", "SKILL.md", 100, None)
                .is_ok()
        );
        frozen.user_message("Please run $example now");
        let first = frozen
            .resource("user/example", "references/large.txt", 10, None)
            .unwrap();
        assert_eq!(first["body"], "中中中");
        assert_eq!(first["cursor"], "9");
        assert!(read(&skill, "../SKILL.md").is_err());
        std::fs::write(skill.root.join("SKILL.md"), b"changed").unwrap();
        assert!(read(&skill, "SKILL.md").is_err());
        let manifest = skill.root.with_extension("manifest.json");
        std::fs::write(
            manifest,
            serde_json::to_vec(&Manifest {
                files: BTreeMap::from([(
                    "SKILL.md".into(),
                    blake3::hash(b"changed").to_hex().to_string(),
                )]),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(read(&skill, "SKILL.md").is_err());
    }
}

#[derive(Serialize, Deserialize)]
struct Upload {
    id: String,
    expected_revision: u64,
}
pub(crate) fn upload_begin(
    root: &Path,
    owner: &str,
    id: &str,
    expected_revision: u64,
) -> Result<Value> {
    ai_terminal_agent_runtime::extensions::user_id(id)?;
    let directory = root.join("runtime/uploads").join(owner_key(owner));
    crate::service::secure_dir(&directory)?;
    let mut active = 0;
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir()
            && metadata.modified()?.elapsed().unwrap_or_default() > Duration::from_secs(3600)
        {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            active += 1;
        }
    }
    ensure!(active < 4, "skill_upload_limit");
    let token = format!("{:016x}{:016x}", crate::random_id(), crate::random_id());
    let path = directory.join(&token);
    crate::service::secure_dir(&path.join("files"))?;
    let mut file = crate::service::open_private(&path.join("upload.json"), false)?;
    serde_json::to_writer(
        &mut file,
        &Upload {
            id: id.into(),
            expected_revision,
        },
    )?;
    file.sync_all()?;
    Ok(json!({"upload_id":token,"chunk_bytes":49152,"max_package_bytes":8388608}))
}
fn upload_path(root: &Path, owner: &str, token: &str) -> Result<PathBuf> {
    ensure!(
        token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid_upload_id"
    );
    let path = root
        .join("runtime/uploads")
        .join(owner_key(owner))
        .join(token);
    ensure!(path.join("upload.json").is_file(), "upload_not_found");
    Ok(path)
}
pub(crate) fn upload_chunk(
    root: &Path,
    owner: &str,
    token: &str,
    path: &str,
    offset: u64,
    data: &str,
) -> Result<Value> {
    use base64::Engine;
    use std::io::{Read, Seek, SeekFrom};
    ensure!(data.len() <= 65536, "skill_chunk_limit");
    let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
    ensure!(
        offset + bytes.len() as u64 <= 2 * 1024 * 1024,
        "skill_file_limit"
    );
    let directory = upload_path(root, owner, token)?
        .join("files")
        .canonicalize()?;
    let mut files = BTreeMap::new();
    collect(&directory, &directory, &mut files, 0)?;
    ensure!(
        files.values().map(Vec::len).sum::<usize>() + bytes.len() <= 8 * 1024 * 1024,
        "skill_package_limit"
    );
    let target = directory.join(relative(path)?);
    crate::service::secure_dir(target.parent().unwrap())?;
    let mut file = crate::service::open_private(&target, false)?;
    let length = file.metadata()?.len();
    if offset < length {
        ensure!(
            offset + bytes.len() as u64 <= length,
            "upload_offset_conflict"
        );
        file.seek(SeekFrom::Start(offset))?;
        let mut previous = vec![0; bytes.len()];
        file.read_exact(&mut previous)?;
        ensure!(previous == bytes, "upload_chunk_conflict");
    } else {
        ensure!(offset == length, "upload_offset_conflict");
        file.seek(SeekFrom::End(0))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(json!({"upload_id":token,"path":path,"next_offset":offset+bytes.len() as u64}))
}
pub(crate) fn upload_finish(
    root: &Path,
    owner: &str,
    token: &str,
) -> Result<(String, PathBuf, u64, PathBuf)> {
    let path = upload_path(root, owner, token)?;
    let upload: Upload = serde_json::from_slice(&std::fs::read(path.join("upload.json"))?)?;
    Ok((
        upload.id,
        path.join("files"),
        upload.expected_revision,
        path,
    ))
}

pub(crate) fn package_files(skill: &Skill) -> Result<BTreeMap<String, Vec<u8>>> {
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(skill.root.with_extension("manifest.json"))?)?;
    manifest
        .files
        .keys()
        .map(|name| Ok((name.clone(), read(skill, name)?)))
        .collect()
}

#[cfg(test)]
mod upload_contracts {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[test]
    fn chunks_are_idempotent_owner_scoped_and_commit_publishes_one_revision() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let config = crate::config::ConfigService::open(&root).unwrap();
        let start = config
            .execute(
                "owner",
                crate::config::Command::SkillUploadBegin {
                    id: "sample".into(),
                    expected_revision: 0,
                },
            )
            .unwrap();
        let token = start["upload_id"].as_str().unwrap();
        let body = b"---\nname: sample\ndescription: Uploaded sample\n---\nRead me";
        for (offset, bytes) in [(0, &body[..20]), (20, &body[20..]), (0, &body[..20])] {
            config
                .execute(
                    "owner",
                    crate::config::Command::SkillUploadChunk {
                        upload_id: token.into(),
                        path: "SKILL.md".into(),
                        offset,
                        data: STANDARD.encode(bytes),
                    },
                )
                .unwrap();
        }
        assert!(upload_chunk(&root, "other", token, "SKILL.md", 0, "YQ==").is_err());
        assert!(upload_chunk(&root, "owner", token, "../escape", 0, "YQ==").is_err());
        assert!(upload_chunk(&root, "owner", token, "SKILL.md", 0, "YQ==").is_err());
        let result = config
            .execute(
                "owner",
                crate::config::Command::SkillUploadCommit {
                    upload_id: token.into(),
                },
            )
            .unwrap();
        assert_eq!(result["revision"], 1);
        let view = config.snapshot("owner");
        assert_eq!(
            read(&view.config.skills["sample"], "SKILL.md").unwrap(),
            body
        );
        assert!(
            config
                .execute(
                    "owner",
                    crate::config::Command::SkillUploadBegin {
                        id: "builtin/terminal-visual".into(),
                        expected_revision: 1
                    }
                )
                .is_err()
        );
    }
}

/// Called only during daemon startup, before any run can hold a package version.
pub(crate) fn collect_old_versions(
    root: &Path,
    retained: &std::collections::HashSet<PathBuf>,
) -> Result<()> {
    let directory = root.join("skills");
    if !directory.exists() {
        return Ok(());
    }
    let mut visited = 0;
    for owner in std::fs::read_dir(directory)?.take(128) {
        let owner = owner?;
        if !owner.file_type()?.is_dir() {
            continue;
        }
        for package in std::fs::read_dir(owner.path())?.take(256) {
            let package = package?;
            if !package.file_type()?.is_dir() {
                continue;
            }
            for version in std::fs::read_dir(package.path())?.take(256) {
                visited += 1;
                if visited > 4096 {
                    return Ok(());
                }
                let version = version?;
                if !version.file_type()?.is_dir() {
                    continue;
                }
                let name = version.file_name().to_string_lossy().into_owned();
                if !(name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
                    || name.contains(".staging-"))
                {
                    continue;
                }
                if retained.contains(&version.path()) {
                    continue;
                }
                if version
                    .metadata()?
                    .modified()?
                    .elapsed()
                    .unwrap_or_default()
                    < Duration::from_secs(7 * 86400)
                {
                    continue;
                }
                std::fs::remove_dir_all(version.path())?;
                let manifest = version.path().with_extension("manifest.json");
                if manifest.is_file() {
                    std::fs::remove_file(manifest)?;
                }
            }
        }
    }
    Ok(())
}
