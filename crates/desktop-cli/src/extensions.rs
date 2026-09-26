use ai_terminal_agent::{
    Client,
    config::{Command, View},
};
use ai_terminal_agent_runtime::extensions::McpServer;
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Context, Result, bail, ensure};
use clap::Subcommand;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Subcommand)]
pub enum Action {
    List,
    Show {
        id: String,
    },
    Add {
        path: Option<PathBuf>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        id: Option<String>,
    },
    Update {
        id: String,
        #[arg(long)]
        file: PathBuf,
    },
    Remove {
        id: String,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Validate {
        id: String,
    },
}
fn call(client: &Client, command: Command) -> Result<Value> {
    let reply = client.call(Request {
        operation: Operation::Configuration as i32,
        text: serde_json::to_string(&command)?,
        ..Default::default()
    })?;
    Ok(serde_json::from_str(
        reply
            .history
            .first()
            .context("missing_configuration_response")?,
    )?)
}
pub fn run(kind: &str, action: Action, state: Option<PathBuf>, json_output: bool) -> Result<u32> {
    let root = state
        .map(Ok)
        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
    let client = Client::ensure(&root, &std::env::current_exe()?)?;
    let mut view: View = serde_json::from_value(call(&client, Command::Show)?)?;
    let result = match action {
        Action::List => {
            if kind == "mcp" {
                json!({"builtin":[{"id":"builtin/terminal","read_only":true}],"user":view.config.mcp})
            } else {
                json!({"builtin":ai_terminal_agent::extensions::builtin_catalog(),"user":view.config.skills,"sources":view.config.skill_sources})
            }
        }
        Action::Show { id } if id.starts_with("builtin/") => {
            if kind == "mcp" {
                ensure!(id == "builtin/terminal", "mcp_not_found");
                json!({"id":id,"read_only":true})
            } else {
                ai_terminal_agent::extensions::builtin_catalog()
                    .into_iter()
                    .find(|s| s["id"] == id)
                    .context("skill_not_found")?
            }
        }
        Action::Show { id } => {
            if kind == "mcp" {
                serde_json::to_value(view.config.mcp.get(&id).context("mcp_not_found")?)?
            } else {
                call(
                    &client,
                    Command::SkillRead {
                        id,
                        path: "SKILL.md".into(),
                    },
                )?
            }
        }
        Action::Validate { id } => {
            if kind == "mcp" {
                call(&client, Command::McpValidate { id })?
            } else {
                call(
                    &client,
                    Command::SkillRead {
                        id,
                        path: "SKILL.md".into(),
                    },
                )?
            }
        }
        Action::Add { path, file, id } if kind == "skills" => {
            let path = path
                .or(file)
                .context("skill_path_required")?
                .canonicalize()?;
            let id = id
                .or_else(|| path.file_name().map(|s| s.to_string_lossy().into_owned()))
                .context("skill_id_required")?;
            call(
                &client,
                Command::SkillInstall {
                    id,
                    path,
                    expected_revision: view.revision,
                },
            )?
        }
        Action::Add { path, file, id } => {
            ensure!(path.is_none() && id.is_none(), "mcp_add_requires_file");
            let servers = read_servers(file.context("mcp_file_required")?)?;
            for (id, server) in servers {
                ensure!(!view.config.mcp.contains_key(&id), "mcp_already_exists");
                view.config.mcp.insert(id, server);
            }
            replace(&client, view)?
        }
        Action::Update { id, file } => {
            ai_terminal_agent_runtime::extensions::user_id(&id)?;
            if kind == "skills" {
                call(
                    &client,
                    Command::SkillInstall {
                        id,
                        path: file.canonicalize()?,
                        expected_revision: view.revision,
                    },
                )?
            } else {
                ensure!(view.config.mcp.contains_key(&id), "mcp_not_found");
                let mut servers = read_servers(file)?;
                ensure!(
                    servers.len() == 1 && servers.contains_key(&id),
                    "mcp_update_id_mismatch"
                );
                view.config
                    .mcp
                    .insert(id.clone(), servers.remove(&id).unwrap());
                replace(&client, view)?
            }
        }
        operation => {
            let (id, enabled) = match operation {
                Action::Remove { id } => (id, None),
                Action::Enable { id } => (id, Some(true)),
                Action::Disable { id } => (id, Some(false)),
                _ => unreachable!(),
            };
            ai_terminal_agent_runtime::extensions::user_id(&id)?;
            if kind == "mcp" {
                let item = view.config.mcp.get_mut(&id).context("mcp_not_found")?;
                if let Some(enabled) = enabled {
                    item.enabled = enabled
                } else {
                    view.config.mcp.remove(&id);
                }
            } else {
                let id = id.strip_prefix("user/").unwrap_or(&id).to_owned();
                let item = view.config.skills.get_mut(&id).context("skill_not_found")?;
                if let Some(enabled) = enabled {
                    item.enabled = enabled
                } else {
                    view.config.skills.remove(&id);
                }
            }
            replace(&client, view)?
        }
    };
    if json_output {
        println!("{}", json!({"ok":true,"result":result}))
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?)
    }
    Ok(0)
}
fn replace(client: &Client, view: View) -> Result<Value> {
    call(
        client,
        Command::Replace {
            expected_revision: view.revision,
            config: view.config,
            secrets: Default::default(),
        },
    )
}
fn read_servers(path: PathBuf) -> Result<BTreeMap<String, McpServer>> {
    ensure!(
        std::fs::metadata(&path)?.len() <= 1024 * 1024,
        "mcp_config_limit"
    );
    let mut value: Value = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|_| anyhow::anyhow!("invalid_mcp_json"))?;
    let object = value.as_object_mut().context("invalid_mcp_json")?;
    if let Some(version) = object.remove("schema_version") {
        ensure!(version == 1, "unsupported_mcp_schema");
    }
    let servers = object
        .remove("mcpServers")
        .context("mcp_servers_required")?;
    if !object.is_empty() {
        bail!("unknown_mcp_config_field");
    }
    serde_json::from_value(servers).map_err(|_| anyhow::anyhow!("invalid_mcp_config"))
}
