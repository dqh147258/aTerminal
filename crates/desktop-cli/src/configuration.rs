use ai_terminal_agent::{
    Client,
    config::{Command as ConfigCommand, View},
};
use ai_terminal_agent_runtime::config::{Binding, ModelProfile, OwnerConfig, Provider};
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Subcommand};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Read or change the raw head/tail line counts retained with terminal observations.
    TerminalReading {
        #[arg(long)]
        head_lines: Option<usize>,
        #[arg(long)]
        tail_lines: Option<usize>,
    },
    ImportLegacyEnv,
    Show,
    Validate,
    Reload,
    Migrate {
        #[arg(long)]
        from: PathBuf,
    },
    Import {
        #[arg(long)]
        file: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum ResourceAction {
    List {
        #[arg(long)]
        provider: Option<String>,
    },
    Discover {
        #[arg(long)]
        provider: String,
        #[arg(long, default_value = "")]
        search: String,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        refresh: bool,
    },
    Show {
        id: String,
    },
    Add(Edit),
    Update(Edit),
    Remove {
        id: String,
    },
    Validate {
        id: String,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    SetDefault {
        id: String,
        #[arg(long, conflicts_with = "session")]
        scope: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
}
#[derive(Args)]
pub struct Edit {
    pub id: Option<String>,
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub non_interactive: bool,
    #[arg(long = "type")]
    pub provider_type: Option<String>,
    #[arg(long)]
    pub base_url: Option<String>,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub context_window: Option<u64>,
    #[arg(long)]
    pub max_tokens: Option<u64>,
    #[arg(long)]
    pub temperature: Option<f64>,
    #[arg(long)]
    pub top_p: Option<f64>,
    #[arg(long)]
    pub reasoning_mode: Option<String>,
    #[arg(long, conflicts_with = "reasoning_budget")]
    pub reasoning_level: Option<String>,
    #[arg(long, conflicts_with = "reasoning_level")]
    pub reasoning_budget: Option<u64>,
    #[arg(long)]
    pub api_key_stdin: bool,
}
fn client(state: Option<PathBuf>) -> Result<Client> {
    let root = state
        .map(Ok)
        .unwrap_or_else(ai_terminal_agent::default_state_dir)?;
    Client::ensure(&root, &std::env::current_exe()?)
}
fn call(client: &Client, command: &ConfigCommand) -> Result<View> {
    let reply = client.call(Request {
        operation: Operation::Configuration as i32,
        text: serde_json::to_string(command)?,
        ..Default::default()
    })?;
    Ok(serde_json::from_str(
        reply
            .history
            .first()
            .context("missing_configuration_response")?,
    )?)
}
fn print(value: impl serde::Serialize, json: bool) -> Result<u32> {
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({"ok":true,"result":value}))?
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(0)
}
pub fn config(action: ConfigAction, state: Option<PathBuf>, json: bool) -> Result<u32> {
    if let ConfigAction::Migrate { from } = action {
        let root = state
            .map(Ok)
            .unwrap_or_else(ai_terminal_agent::state::home_root)?;
        ai_terminal_agent::state::migrate(&from, &root)?;
        return print(
            serde_json::json!({"migrated_to":root,"source_preserved":true}),
            json,
        );
    }
    let client = client(state)?;
    let cmd = match action {
        ConfigAction::TerminalReading {
            head_lines,
            tail_lines,
        } => {
            let mut view = call(&client, &ConfigCommand::Show)?;
            if head_lines.is_some() || tail_lines.is_some() {
                if let Some(count) = head_lines {
                    view.config.terminal_reading.head_lines = count;
                }
                if let Some(count) = tail_lines {
                    view.config.terminal_reading.tail_lines = count;
                }
                view.config.terminal_reading.validate()?;
                view = call(
                    &client,
                    &ConfigCommand::Replace {
                        expected_revision: view.revision,
                        config: view.config,
                        secrets: Default::default(),
                    },
                )?;
            }
            return print(
                serde_json::json!({"revision":view.revision,"terminal_reading":view.config.terminal_reading}),
                json,
            );
        }
        ConfigAction::ImportLegacyEnv => {
            let mut old = call(&client, &ConfigCommand::Show)?;
            ensure!(
                !old.config.providers.contains_key("legacy-env")
                    && !old.config.models.contains_key("legacy-env"),
                "legacy_environment_already_imported"
            );
            let endpoint =
                std::env::var("AI_TERMINAL_AI_BASE_URL").context("legacy_base_url_missing")?;
            let model = std::env::var("AI_TERMINAL_AI_MODEL").context("legacy_model_missing")?;
            let provider: Provider = serde_json::from_value(
                serde_json::json!({"id":"legacy-env","name":"Imported legacy environment","connection":{"protocol":"openai_chat","endpoint":endpoint},"credential_revision":0,"enabled":true}),
            )?;
            let profile: ModelProfile = serde_json::from_value(
                serde_json::json!({"id":"legacy-env","name":"Imported legacy environment","provider_id":"legacy-env","model":model,"context_window":32768,"max_tokens":4096,"read_only":true,"capabilities":{"source":"legacy_unknown"}}),
            )?;
            old.config.providers.insert("legacy-env".into(), provider);
            old.config.models.insert("legacy-env".into(), profile);
            let mut secrets = std::collections::BTreeMap::new();
            if let Ok(secret) = std::env::var("AI_TERMINAL_AI_API_KEY") {
                secrets.insert("legacy-env".into(), secret);
            }
            ConfigCommand::Replace {
                expected_revision: old.revision,
                config: old.config,
                secrets,
            }
        }
        ConfigAction::Show => ConfigCommand::Show,
        ConfigAction::Validate => ConfigCommand::Validate,
        ConfigAction::Reload => ConfigCommand::Reload,
        ConfigAction::Import { file } => {
            let old = call(&client, &ConfigCommand::Show)?;
            let config: OwnerConfig = read_file(file)?;
            ConfigCommand::Replace {
                expected_revision: old.revision,
                config,
                secrets: Default::default(),
            }
        }
        ConfigAction::Migrate { .. } => unreachable!(),
    };
    print(call(&client, &cmd)?, json)
}
fn read_file<T: serde::de::DeserializeOwned>(file: PathBuf) -> Result<T> {
    ensure!(
        std::fs::metadata(&file)?.len() <= 1024 * 1024,
        "config_file_limit"
    );
    Ok(serde_json::from_slice(&std::fs::read(file)?)?)
}
pub fn resource(
    resource: &str,
    action: ResourceAction,
    state: Option<PathBuf>,
    json: bool,
) -> Result<u32> {
    let updating = matches!(&action, ResourceAction::Update(_));
    let enabling = matches!(&action, ResourceAction::Enable { .. });
    let client = client(state)?;
    let mut view = call(&client, &ConfigCommand::Show)?;
    let mut secrets = std::collections::BTreeMap::new();
    let mut default_binding = None;
    let list = if resource == "providers" {
        serde_json::to_value(&view.config.providers)?
    } else {
        serde_json::to_value(&view.config.models)?
    };
    match action {
        ResourceAction::Discover {
            provider,
            search,
            cursor,
            refresh,
        } => {
            ensure!(resource == "models", "invalid_resource_action");
            return print(
                call_value(
                    &client,
                    &ConfigCommand::Discover {
                        provider,
                        search,
                        cursor,
                        refresh,
                    },
                )?,
                json,
            );
        }
        ResourceAction::List { provider } => {
            let list = if let Some(provider) = provider {
                ensure!(resource == "models", "provider_filter_requires_models");
                serde_json::to_value(
                    view.config
                        .models
                        .iter()
                        .filter(|(_, m)| m.provider_id == provider)
                        .collect::<std::collections::BTreeMap<_, _>>(),
                )?
            } else {
                list
            };
            return print(list, json);
        }
        ResourceAction::Show { id } => {
            let item = list.get(&id).context("item_not_found")?;
            return print(item, json);
        }
        ResourceAction::Validate { id } => {
            ensure!(list.get(&id).is_some(), "item_not_found");
            view.config.validate()?;
            return print(serde_json::json!({"id":id,"valid":true}), json);
        }
        ResourceAction::Remove { id } => {
            if resource == "providers" {
                ensure!(
                    view.config.providers.remove(&id).is_some(),
                    "provider_not_found"
                );
            } else {
                ensure!(view.config.models.remove(&id).is_some(), "model_not_found");
            }
        }
        ResourceAction::Enable { id } | ResourceAction::Disable { id } => {
            ensure!(resource == "providers", "invalid_resource_action");
            view.config
                .providers
                .get_mut(&id)
                .context("provider_not_found")?
                .enabled = enabling;
        }
        ResourceAction::SetDefault { id, scope, session } => {
            ensure!(resource == "models", "invalid_resource_action");
            ensure!(view.config.models.contains_key(&id), "model_not_found");
            let scope = if let Some(session) = session {
                format!("session/{session}")
            } else {
                let scope = scope.unwrap_or_else(|| "global".into());
                ensure!(
                    ["global", "session-default"].contains(&scope.as_str()),
                    "invalid_binding_scope"
                );
                scope
            };
            view.config.bindings.insert(
                scope,
                Binding {
                    model_id: id,
                    reasoning: None,
                },
            );
        }
        ResourceAction::Add(edit) | ResourceAction::Update(edit) => {
            let existing = edit.id.as_ref().and_then(|id| list.get(id)).cloned();
            let has_flags = edit.provider_type.is_some()
                || edit.base_url.is_some()
                || edit.provider.is_some()
                || edit.model.is_some()
                || edit.context_window.is_some()
                || edit.max_tokens.is_some()
                || edit.temperature.is_some()
                || edit.top_p.is_some()
                || edit.reasoning_mode.is_some()
                || edit.reasoning_level.is_some()
                || edit.reasoning_budget.is_some()
                || edit.api_key_stdin;
            let mut value = if let Some(file) = &edit.file {
                read_file::<serde_json::Value>(file.clone())?
            } else if has_flags {
                existing.unwrap_or_else(|| draft(resource, edit.id.as_deref()))
            } else {
                ensure!(
                    !edit.non_interactive && !json && io::stdin().is_terminal(),
                    "non_interactive_requires_file"
                );
                wizard(
                    resource,
                    edit.id.as_deref(),
                    existing,
                    &client,
                    &view.config,
                    &mut secrets,
                    &mut default_binding,
                )?
            };
            apply_fields(resource, &edit, &mut value, updating)?;
            if edit.api_key_stdin {
                ensure!(resource == "providers", "API keys belong to providers");
                let mut secret = String::new();
                ensure!(io::stdin().read_line(&mut secret)? > 0, "api_key_required");
                let secret = secret.trim_end_matches(['\r', '\n']).to_owned();
                ensure!(!secret.is_empty(), "api_key_required");
                secrets.insert(
                    value["id"].as_str().context("id_required")?.to_owned(),
                    secret,
                );
            }
            let item_id = value["id"].as_str().context("id_required")?;
            ensure!(
                list.get(item_id).is_some() == updating,
                if updating {
                    "item_not_found"
                } else {
                    "item_already_exists"
                }
            );
            if resource == "providers" {
                let provider: Provider = serde_json::from_value(value)?;
                if let Some(id) = edit.id {
                    ensure!(provider.id == id, "provider_id_mismatch");
                }
                view.config.providers.insert(provider.id.clone(), provider);
            } else {
                let model: ModelProfile = serde_json::from_value(value)?;
                if let Some(id) = edit.id {
                    ensure!(model.id == id, "model_id_mismatch");
                }
                if let Some(scope) = default_binding.take() {
                    view.config.bindings.insert(
                        scope,
                        Binding {
                            model_id: model.id.clone(),
                            reasoning: None,
                        },
                    );
                }
                view.config.models.insert(model.id.clone(), model);
            }
        }
    }
    view.config.validate()?;
    print(
        call(
            &client,
            &ConfigCommand::Replace {
                expected_revision: view.revision,
                config: view.config,
                secrets,
            },
        )?,
        json,
    )
}
fn call_value(client: &Client, command: &ConfigCommand) -> Result<serde_json::Value> {
    let reply = client.call(Request {
        operation: Operation::Configuration as i32,
        text: serde_json::to_string(command)?,
        ..Default::default()
    })?;
    Ok(serde_json::from_str(
        reply
            .history
            .first()
            .context("missing_configuration_response")?,
    )?)
}
fn wizard(
    resource: &str,
    id: Option<&str>,
    existing: Option<serde_json::Value>,
    client: &Client,
    config: &OwnerConfig,
    secrets: &mut std::collections::BTreeMap<String, String>,
    binding: &mut Option<String>,
) -> Result<serde_json::Value> {
    let mut candidate = existing.unwrap_or_else(|| draft(resource, id));
    let mut secret = None;
    let mut chosen_binding: Option<String> = None;
    let mut step = 0usize;
    loop {
        let fields = if resource == "providers" {
            let mut fields = vec![
                ("/id", "ID"),
                (
                    "/connection/protocol",
                    "Protocol: openai_responses/openai_chat/anthropic/gemini/azure_openai/ollama",
                ),
                ("/name", "Name"),
                ("/connection/endpoint", "Endpoint"),
            ];
            if candidate["connection"]["protocol"] == "azure_openai" {
                fields.push(("/connection/api_version", "Azure API version"));
            }
            fields.push((
                "secret",
                "API key (hidden; blank keeps existing credential)",
            ));
            fields
        } else {
            vec![
                ("/id", "ID"),
                ("/provider_id", "Provider ID"),
                ("/model", "Model selection"),
                ("/name", "Name"),
                ("/context_window", "Effective context budget (tokens)"),
                ("/max_tokens", "Output budget (tokens)"),
                (
                    "tools",
                    "Tools: yes / no / unknown; yes is your explicit capability declaration",
                ),
                ("reasoning", "Reasoning"),
                (
                    "binding",
                    "Default binding: none / global / session-default / session/ID",
                ),
            ]
        };
        step = step.min(fields.len());
        if step == fields.len() {
            let validation = (|| -> Result<()> {
                let mut config = config.clone();
                if resource == "providers" {
                    let p: Provider = serde_json::from_value(candidate.clone())?;
                    config.providers.insert(p.id.clone(), p);
                } else {
                    let m: ModelProfile = serde_json::from_value(candidate.clone())?;
                    if let Some(scope) = &chosen_binding {
                        config.bindings.insert(
                            scope.clone(),
                            Binding {
                                model_id: m.id.clone(),
                                reasoning: None,
                            },
                        );
                    }
                    config.models.insert(m.id.clone(), m);
                }
                config.validate()
            })();
            if let Err(ref error) = validation {
                eprintln!("Configuration error: {error}. Use back to correct the draft.");
            }
            eprintln!("{}", serde_json::to_string_pretty(&candidate)?);
            eprintln!(
                "Credential: {}; default: {}",
                if secret.is_some() {
                    "new credential (hidden)"
                } else {
                    "unchanged"
                },
                chosen_binding.as_deref().unwrap_or("unchanged")
            );
            match prompt("Submit: yes / back / cancel", "")?.as_str() {
                "yes" if validation.is_ok() => {
                    if let Some(secret) = secret {
                        secrets.insert(
                            candidate["id"].as_str().context("id_required")?.into(),
                            secret,
                        );
                    }
                    *binding = chosen_binding;
                    return Ok(candidate);
                }
                "back" | ":back" => step = step.saturating_sub(1),
                "cancel" | ":cancel" => bail!("cancelled"),
                _ => {}
            }
            continue;
        }
        let (path, label) = fields[step];
        if path == "/provider_id" {
            eprintln!(
                "Providers: {}",
                config
                    .providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if path == "/model" {
            let provider = candidate["provider_id"]
                .as_str()
                .context("provider_required")?;
            match choose_model(client, provider)? {
                None => {
                    step = step.saturating_sub(1);
                    continue;
                }
                Some(model) => {
                    candidate["model"] = model["id"].clone();
                    candidate["capabilities"] =
                        model
                            .get("capabilities")
                            .cloned()
                            .unwrap_or(serde_json::to_value(
                                ai_terminal_agent_runtime::config::Capabilities::default(),
                            )?);
                    for (key, source) in [
                        ("context_window", "context_window"),
                        ("max_tokens", "max_output_tokens"),
                    ] {
                        if model[source].is_u64() {
                            candidate[key] = model[source].clone();
                        }
                    }
                    candidate["reasoning"] = serde_json::json!({"mode":"provider_default"});
                    step += 1;
                    continue;
                }
            }
        }
        if path == "reasoning" {
            let caps: ai_terminal_agent_runtime::config::Capabilities =
                serde_json::from_value(candidate["capabilities"].clone())?;
            let mut choices = vec!["provider_default".to_owned()];
            if caps.reasoning_disabled {
                choices.push("disabled".into());
            }
            if caps.reasoning_adaptive {
                choices.push("adaptive".into());
            }
            choices.extend(caps.reasoning_levels.iter().map(|l| format!("level:{l}")));
            if let Some((min, max)) = caps.reasoning_budget {
                choices.push(format!("budget:N ({min}..{max})"));
            }
            eprintln!("Available reasoning: {}", choices.join(", "));
        }
        let old = candidate.pointer(path).cloned().unwrap_or_default();
        let default = old.as_str().map(str::to_owned).unwrap_or_else(|| {
            if old.is_null() {
                String::new()
            } else {
                old.to_string()
            }
        });
        let answer = if path == "secret" {
            rpassword::prompt_password(format!("{label} (:back / :cancel): "))?
        } else {
            prompt(&format!("{label} (:back / :cancel)"), &default)?
        };
        match answer.as_str() {
            ":cancel" => bail!("cancelled"),
            ":back" => {
                step = step.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        let change = (|| -> Result<()> {
            match path {
                "secret" => {
                    secret = if answer.is_empty() {
                        None
                    } else {
                        Some(answer.clone())
                    };
                }
                "binding" => {
                    ensure!(
                        answer == "none"
                            || answer == "global"
                            || answer == "session-default"
                            || answer.starts_with("session/"),
                        "invalid_binding"
                    );
                    chosen_binding = if answer == "none" {
                        None
                    } else {
                        Some(answer.clone())
                    };
                }
                "tools" => {
                    ensure!(
                        ["yes", "no", "unknown"].contains(&answer.as_str()),
                        "invalid_capability_choice"
                    );
                    candidate["capabilities"]["tools"] = match answer.as_str() {
                        "yes" => serde_json::json!(true),
                        "no" => serde_json::json!(false),
                        _ => serde_json::Value::Null,
                    };
                    candidate["read_only"] = serde_json::json!(answer != "yes");
                    candidate["capabilities"]["source"] = serde_json::json!("user_configuration");
                }
                "reasoning" => {
                    candidate["reasoning"] = if let Some(level) = answer.strip_prefix("level:") {
                        serde_json::json!({"mode":"level","level":level})
                    } else if let Some(tokens) = answer.strip_prefix("budget:") {
                        serde_json::json!({"mode":"budget","tokens":tokens.parse::<u64>()?})
                    } else {
                        serde_json::json!({"mode":answer})
                    };
                    let model: ModelProfile = serde_json::from_value(candidate.clone())?;
                    let provider = config
                        .providers
                        .get(&model.provider_id)
                        .context("provider_not_found")?;
                    model.settings(&provider.connection.protocol, &model.reasoning)?;
                }
                _ => {
                    let value = if old.is_number() {
                        serde_json::json!(answer.parse::<u64>()?)
                    } else if answer.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!(answer)
                    };
                    if path == "/id" {
                        ai_terminal_agent_runtime::config::valid_id(&answer)?;
                        if let Some(id) = id {
                            ensure!(id == answer, "ID cannot change during update");
                        }
                    }
                    if path == "/connection/protocol" {
                        let _: ai_terminal_agent_runtime::model::Protocol =
                            serde_json::from_value(value.clone())?;
                    }
                    if path == "/provider_id" {
                        ensure!(config.providers.contains_key(&answer), "provider_not_found");
                    }
                    *candidate
                        .pointer_mut(path)
                        .context("invalid_wizard_field")? = value;
                }
            }
            Ok(())
        })();
        match change {
            Ok(()) => step += 1,
            Err(error) => eprintln!("{error}"),
        }
    }
}
fn choose_model(client: &Client, provider: &str) -> Result<Option<serde_json::Value>> {
    let mut search = String::new();
    let mut cursor = None;
    let mut refresh = false;
    loop {
        let result = call_value(
            client,
            &ConfigCommand::Discover {
                provider: provider.into(),
                search: search.clone(),
                cursor: cursor.clone(),
                refresh,
            },
        );
        let models = match &result {
            Ok(page) => {
                eprintln!(
                    "Model catalog: {}{}",
                    if page["cached"] == true {
                        "cached"
                    } else {
                        "provider"
                    },
                    if page["stale"] == true {
                        " (stale)"
                    } else {
                        ""
                    }
                );
                page["models"].as_array().cloned().unwrap_or_default()
            }
            Err(error) => {
                eprintln!("Catalog unavailable: {error}");
                Vec::new()
            }
        };
        for (i, model) in models.iter().enumerate() {
            eprintln!("{}. {}", i + 1, model["id"].as_str().unwrap_or(""));
        }
        let answer = prompt(
            "Choose number / manual MODEL_ID / search TEXT / next / refresh / :back / :cancel",
            "",
        )?;
        if answer == ":cancel" {
            bail!("cancelled")
        }
        if answer == ":back" {
            return Ok(None);
        }
        if let Some(id) = answer.strip_prefix("manual ") {
            ensure!(!id.is_empty(), "model_required");
            return Ok(Some(serde_json::json!({"id":id})));
        }
        if let Ok(index) = answer.parse::<usize>()
            && index > 0
            && let Some(model) = models.get(index - 1)
        {
            return Ok(Some(model.clone()));
        }
        if let Some(query) = answer.strip_prefix("search ") {
            search = query.into();
            cursor = None;
            refresh = false;
        } else if answer == "next" {
            cursor = result
                .as_ref()
                .ok()
                .and_then(|page| page["cursor"].as_str().map(str::to_owned));
            refresh = false;
            if cursor.is_none() {
                eprintln!("No more pages");
            }
        } else if answer == "refresh" {
            cursor = None;
            refresh = true;
        }
    }
}
fn prompt(label: &str, default: &str) -> Result<String> {
    eprint!("{label} [{default}]: ");
    io::stderr().flush()?;
    let mut line = String::new();
    ensure!(io::stdin().read_line(&mut line)? > 0, "cancelled");
    let line = line.trim();
    Ok(if line.is_empty() {
        default.into()
    } else {
        line.into()
    })
}

fn draft(resource: &str, id: Option<&str>) -> serde_json::Value {
    if resource == "providers" {
        serde_json::json!({"id":id.unwrap_or("main"),"name":"Main","connection":{"protocol":"openai_chat","endpoint":"https://api.openai.com/v1","api_version":null},"catalog_url":null,"secret_ref":null,"credential_revision":0,"enabled":true})
    } else {
        serde_json::json!({"id":id.unwrap_or("main-agent"),"name":"Main Agent","provider_id":"main","model":"","context_window":32000,"max_tokens":4096,"temperature":null,"top_p":null,"reasoning":{"mode":"provider_default"},"capabilities":{"tools":null,"vision":null,"streaming":null},"max_rounds":24,"max_seconds":300,"read_only":true})
    }
}
fn apply_fields(
    resource: &str,
    edit: &Edit,
    value: &mut serde_json::Value,
    updating: bool,
) -> Result<()> {
    if resource == "providers" {
        if let Some(kind) = &edit.provider_type {
            let protocol = match kind.as_str() {
                "openai" => "openai_responses",
                "openai-compatible" => "openai_chat",
                s => s,
            };
            ensure!(
                !updating || value["connection"]["protocol"] == protocol || edit.base_url.is_some(),
                "changing protocol requires --base-url"
            );
            value["connection"]["protocol"] = serde_json::json!(protocol);
            if !updating && edit.base_url.is_none() {
                value["connection"]["endpoint"] = serde_json::json!(match protocol {
                    "openai_chat" | "openai_responses" => "https://api.openai.com/v1",
                    "anthropic" => "https://api.anthropic.com",
                    "gemini" => "https://generativelanguage.googleapis.com",
                    "ollama" => "http://127.0.0.1:11434",
                    "azure_openai" => "",
                    _ => bail!("unknown_provider_protocol"),
                });
            }
        }
        if let Some(url) = &edit.base_url {
            value["connection"]["endpoint"] = serde_json::json!(url);
        }
    } else {
        for (key, v) in [
            ("provider_id", edit.provider.as_ref()),
            ("model", edit.model.as_ref()),
        ] {
            if let Some(v) = v {
                value[key] = serde_json::json!(v);
            }
        }
        for (key, v) in [
            ("context_window", edit.context_window),
            ("max_tokens", edit.max_tokens),
        ] {
            if let Some(v) = v {
                value[key] = serde_json::json!(v);
            }
        }
        for (key, v) in [("temperature", edit.temperature), ("top_p", edit.top_p)] {
            if let Some(v) = v {
                value[key] = serde_json::json!(v);
            }
        }
        if let Some(level) = &edit.reasoning_level {
            ensure!(
                edit.reasoning_mode.as_deref().is_none_or(|v| v == "level"),
                "reasoning_mode_conflict"
            );
            value["reasoning"] = serde_json::json!({"mode":"level","level":level});
        } else if let Some(tokens) = edit.reasoning_budget {
            ensure!(
                edit.reasoning_mode.as_deref().is_none_or(|v| v == "budget"),
                "reasoning_mode_conflict"
            );
            value["reasoning"] = serde_json::json!({"mode":"budget","tokens":tokens});
        } else if let Some(mode) = &edit.reasoning_mode {
            ensure!(
                ["provider_default", "disabled", "adaptive"].contains(&mode.as_str()),
                "reasoning_value_required"
            );
            value["reasoning"] = serde_json::json!({"mode":mode});
        }
    }
    Ok(())
}
