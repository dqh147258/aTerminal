//! Serializable extension configuration, independent of the host's process/file handles.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    #[serde(default)]
    pub transport: Option<String>,
    pub command: Option<PathBuf>,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub url: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, rename = "envSecretRefs")]
    pub env_secret_refs: BTreeMap<String, String>,
    #[serde(default, rename = "headerSecretRefs")]
    pub header_secret_refs: BTreeMap<String, String>,
    #[serde(default = "startup")]
    pub startup_timeout_ms: u64,
    #[serde(default = "timeout")]
    pub call_timeout_ms: u64,
}
fn yes() -> bool {
    true
}
fn startup() -> u64 {
    10000
}
fn timeout() -> u64 {
    30000
}
impl McpServer {
    pub fn wire(&self) -> Result<&str> {
        match self.transport.as_deref() {
            Some("stdio") => Ok("stdio"),
            Some("streamable_http") => Ok("streamable_http"),
            None if self.command.is_some() && self.url.is_none() => Ok("stdio"),
            None if self.url.is_some() && self.command.is_none() => Ok("streamable_http"),
            _ => anyhow::bail!("unsupported_mcp_transport"),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (100..=60000).contains(&self.startup_timeout_ms)
                && (100..=300000).contains(&self.call_timeout_ms),
            "invalid_mcp_timeout"
        );
        match self.wire()? {
            "stdio" => {
                ensure!(
                    self.command
                        .as_ref()
                        .is_some_and(|p| !p.as_os_str().is_empty())
                        && self.url.is_none(),
                    "mcp_command_required"
                );
            }
            _ => {
                let url = reqwest::Url::parse(
                    self.url
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("mcp_url_required"))?,
                )?;
                ensure!(
                    ["http", "https"].contains(&url.scheme())
                        && url.username().is_empty()
                        && url.password().is_none()
                        && self.command.is_none(),
                    "invalid_mcp_url"
                );
            }
        }
        ensure!(
            self.args.len() <= 128
                && self.args.iter().map(String::len).sum::<usize>() <= 16000
                && self.env.len() + self.env_secret_refs.len() <= 128
                && self.header_secret_refs.len() + self.headers.len() <= 32,
            "mcp_config_limit"
        );
        for key in self.env.keys().chain(self.env_secret_refs.keys()) {
            ensure!(
                !key.is_empty() && !key.contains(['=', '\0']),
                "invalid_environment_key"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub root: PathBuf,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub allow_implicit: bool,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub interface: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSource {
    pub id: String,
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub project: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}
pub fn user_id(id: &str) -> Result<()> {
    ensure!(!id.starts_with("builtin"), "builtin_read_only");
    crate::config::valid_id(id.strip_prefix("user/").unwrap_or(id))
}
