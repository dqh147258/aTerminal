//! Public configuration contract. Secrets are write-only Desktop vault entries.
use crate::model::{Connection, Protocol, RequestSettings};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reasoning {
    #[default]
    ProviderDefault,
    Disabled,
    Adaptive,
    Level {
        level: String,
    },
    Budget {
        tokens: u64,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    pub streaming: Option<bool>,
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
    pub reasoning_budget: Option<(u64, u64)>,
    #[serde(default)]
    pub reasoning_disabled: bool,
    #[serde(default)]
    pub reasoning_adaptive: bool,
    pub temperature: Option<bool>,
    pub top_p: Option<bool>,
    pub source: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub connection: Connection,
    pub catalog_url: Option<String>,
    pub secret_ref: Option<String>,
    pub credential_revision: u64,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model: String,
    pub context_window: u64,
    pub max_tokens: u64,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    #[serde(default)]
    pub reasoning: Reasoning,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default = "default_rounds")]
    pub max_rounds: u32,
    #[serde(default = "default_seconds")]
    pub max_seconds: u64,
    #[serde(default)]
    pub read_only: bool,
}
fn default_rounds() -> u32 {
    24
}
fn default_seconds() -> u64 {
    300
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub model_id: String,
    pub reasoning: Option<Reasoning>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalReading {
    pub head_lines: usize,
    pub tail_lines: usize,
}
impl Default for TerminalReading {
    fn default() -> Self {
        Self {
            head_lines: 10,
            tail_lines: 20,
        }
    }
}
impl TerminalReading {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=100).contains(&self.head_lines) && (1..=100).contains(&self.tail_lines),
            "invalid_terminal_anchor_lines"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerConfig {
    #[serde(default)]
    pub terminal_reading: TerminalReading,
    #[serde(default)]
    pub providers: BTreeMap<String, Provider>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelProfile>,
    #[serde(default)]
    pub bindings: BTreeMap<String, Binding>,
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::extensions::McpServer>,
    #[serde(default)]
    pub skills: BTreeMap<String, crate::extensions::Skill>,
    #[serde(default)]
    pub skill_sources: Vec<crate::extensions::SkillSource>,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
}
impl OwnerConfig {
    pub fn validate(&self) -> Result<()> {
        self.terminal_reading.validate()?;
        ensure!(
            self.providers.len() <= 128 && self.models.len() <= 512 && self.bindings.len() <= 4096,
            "config_limit"
        );
        ensure!(
            self.mcp.len() <= 64
                && self.skills.len() <= 128
                && self.skill_sources.len() <= 16
                && self.credentials.len() <= 1024,
            "extension_config_limit"
        );
        for (id, server) in &self.mcp {
            crate::extensions::user_id(id)?;
            server.validate()?;
        }
        for (id, skill) in &self.skills {
            crate::extensions::user_id(id)?;
            ensure!(
                id == &skill.id
                    && skill.version.len() == 64
                    && skill.version.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid_skill_registration"
            );
        }
        for source in &self.skill_sources {
            crate::extensions::user_id(&source.id)?;
            ensure!(
                source.project || source.path.as_ref().is_some_and(|p| p.is_absolute()),
                "skill_source_path_required"
            );
        }
        for (alias, reference) in &self.credentials {
            crate::config::valid_id(alias)?;
            ensure!(
                reference.len() == 32 && reference.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid_credential_reference"
            );
        }
        for (id, p) in &self.providers {
            valid_id(id)?;
            ensure!(id == &p.id, "provider_id_mismatch");
            let url = reqwest::Url::parse(&p.connection.endpoint)?;
            ensure!(
                ["http", "https"].contains(&url.scheme())
                    && url.username().is_empty()
                    && url.password().is_none(),
                "invalid_endpoint"
            );
            ensure!(
                !p.name.is_empty() && p.name.len() <= 256,
                "invalid_provider_name"
            );
            if p.connection.protocol == Protocol::AzureOpenai {
                ensure!(
                    p.connection
                        .api_version
                        .as_ref()
                        .is_some_and(|v| !v.is_empty()),
                    "azure_api_version_required"
                );
            }
            if let Some(url) = &p.catalog_url {
                let url = reqwest::Url::parse(url)?;
                ensure!(
                    ["http", "https"].contains(&url.scheme())
                        && url.username().is_empty()
                        && url.password().is_none(),
                    "invalid_catalog_url"
                );
            }
        }
        for (id, m) in &self.models {
            valid_id(id)?;
            ensure!(id == &m.id, "model_id_mismatch");
            let p = self
                .providers
                .get(&m.provider_id)
                .ok_or_else(|| anyhow::anyhow!("provider_not_found"))?;
            ensure!(
                !m.model.is_empty() && m.model.len() <= 256,
                "model_required"
            );
            ensure!(
                m.context_window >= 4096
                    && m.context_window <= 4_000_000
                    && m.max_tokens > 0
                    && m.max_tokens < m.context_window.saturating_sub(2048),
                "invalid_context_budget"
            );
            ensure!(
                m.max_rounds > 0
                    && m.max_rounds <= 100
                    && m.max_seconds > 0
                    && m.max_seconds <= 3600,
                "invalid_run_budget"
            );
            ensure!(
                m.read_only || m.capabilities.tools == Some(true),
                "tool_capability_required"
            );
            ensure!(
                m.capabilities.streaming != Some(false),
                "streaming_capability_required"
            );
            if let Some(t) = m.temperature {
                ensure!(
                    t.is_finite()
                        && (0.0..=2.0).contains(&t)
                        && m.capabilities.temperature != Some(false),
                    "unsupported_temperature"
                );
            }
            if let Some(t) = m.top_p {
                ensure!(
                    t.is_finite() && t > 0.0 && t <= 1.0 && m.capabilities.top_p != Some(false),
                    "unsupported_top_p"
                );
            }
            m.settings(&p.connection.protocol, &m.reasoning)?;
        }
        for binding in self.bindings.values() {
            let m = self
                .models
                .get(&binding.model_id)
                .ok_or_else(|| anyhow::anyhow!("bound_model_not_found"))?;
            m.settings(
                &self.providers[&m.provider_id].connection.protocol,
                binding.reasoning.as_ref().unwrap_or(&m.reasoning),
            )?;
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        session: Option<&str>,
    ) -> Result<(Provider, ModelProfile, RequestSettings)> {
        let binding = session
            .and_then(|s| self.bindings.get(&format!("session/{s}")))
            .or_else(|| {
                self.bindings.get(if session.is_some() {
                    "session-default"
                } else {
                    "global"
                })
            })
            .ok_or_else(|| anyhow::anyhow!("model_not_configured"))?;
        let model = self
            .models
            .get(&binding.model_id)
            .ok_or_else(|| anyhow::anyhow!("model_not_found"))?
            .clone();
        let provider = self
            .providers
            .get(&model.provider_id)
            .ok_or_else(|| anyhow::anyhow!("provider_not_found"))?
            .clone();
        ensure!(provider.enabled, "provider_disabled");
        let settings = model.settings(
            &provider.connection.protocol,
            binding.reasoning.as_ref().unwrap_or(&model.reasoning),
        )?;
        Ok((provider, model, settings))
    }
}
impl ModelProfile {
    pub fn settings(&self, protocol: &Protocol, reasoning: &Reasoning) -> Result<RequestSettings> {
        let mut params = serde_json::Map::new();
        if let Some(p) = self.top_p {
            params.insert("top_p".into(), json!(p));
        }
        match reasoning {
            Reasoning::ProviderDefault => {}
            Reasoning::Level { level } => {
                ensure!(
                    self.capabilities.reasoning_levels.contains(level),
                    "unsupported_reasoning_level"
                );
                match protocol {
                    Protocol::OpenaiResponses => {
                        params.insert("reasoning".into(), json!({"effort":level}));
                    }
                    Protocol::OpenaiChat | Protocol::AzureOpenai => {
                        params.insert("reasoning_effort".into(), json!(level));
                    }
                    Protocol::Gemini => {
                        params.insert("thinkingConfig".into(), json!({"thinkingLevel":level}));
                    }
                    _ => anyhow::bail!("unsupported_reasoning_mapping"),
                }
            }
            Reasoning::Budget { tokens } => {
                let (min, max) = self
                    .capabilities
                    .reasoning_budget
                    .ok_or_else(|| anyhow::anyhow!("unknown_reasoning_budget"))?;
                ensure!(
                    *tokens >= min && *tokens <= max && *tokens < self.max_tokens,
                    "invalid_reasoning_budget"
                );
                match protocol {
                    Protocol::Anthropic => {
                        ensure!(
                            self.temperature.is_none() && self.top_p.is_none(),
                            "reasoning_sampling_conflict"
                        );
                        params.insert(
                            "thinking".into(),
                            json!({"type":"enabled","budget_tokens":tokens}),
                        );
                    }
                    Protocol::Gemini => {
                        params.insert("thinkingConfig".into(), json!({"thinkingBudget":tokens}));
                    }
                    _ => anyhow::bail!("unsupported_reasoning_mapping"),
                }
            }
            Reasoning::Adaptive => {
                ensure!(
                    self.capabilities.reasoning_adaptive && *protocol == Protocol::Anthropic,
                    "unsupported_adaptive_reasoning"
                );
                ensure!(
                    self.temperature.is_none() && self.top_p.is_none(),
                    "reasoning_sampling_conflict"
                );
                params.insert("thinking".into(), json!({"type":"adaptive"}));
            }
            Reasoning::Disabled => {
                ensure!(
                    self.capabilities.reasoning_disabled,
                    "unsupported_reasoning_disabled"
                );
                match protocol {
                    Protocol::Anthropic => {
                        params.insert("thinking".into(), json!({"type":"disabled"}));
                    }
                    Protocol::Gemini => {
                        params.insert("thinkingConfig".into(), json!({"thinkingBudget":0}));
                    }
                    Protocol::Ollama => {
                        params.insert("think".into(), json!(false));
                    }
                    _ => anyhow::bail!("unsupported_reasoning_mapping"),
                }
            }
        }
        if *protocol == Protocol::Gemini && !params.is_empty() {
            if let Some(top_p) = params.remove("top_p") {
                params.insert("topP".into(), top_p);
            }
            params =
                serde_json::Map::from_iter([("generationConfig".into(), Value::Object(params))]);
        }
        Ok(RequestSettings {
            model: self.model.clone(),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            additional_params: if params.is_empty() {
                None
            } else {
                Some(Value::Object(params))
            },
        })
    }
}
pub fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
        "invalid_id"
    );
    ensure!(id != "builtin", "builtin_read_only");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_reasoning_and_reserved_ids_are_rejected() {
        assert!(valid_id("builtin").is_err());
        assert!(valid_id("../../builtin").is_err());
        let m = ModelProfile {
            id: "m".into(),
            name: "M".into(),
            provider_id: "p".into(),
            model: "configured-id".into(),
            context_window: 32000,
            max_tokens: 4000,
            temperature: None,
            top_p: None,
            reasoning: Reasoning::ProviderDefault,
            capabilities: Capabilities::default(),
            max_rounds: 24,
            max_seconds: 300,
            read_only: true,
        };
        assert!(
            m.settings(&Protocol::OpenaiChat, &Reasoning::ProviderDefault)
                .is_ok()
        );
        assert!(
            m.settings(
                &Protocol::OpenaiChat,
                &Reasoning::Level {
                    level: "high".into()
                }
            )
            .is_err()
        );
    }
}
