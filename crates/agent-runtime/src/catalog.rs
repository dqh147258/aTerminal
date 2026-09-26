//! Provider metadata requests only. This module cannot initiate a completion.
use crate::{
    config::{Capabilities, Provider},
    model::Protocol,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub capabilities: Capabilities,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogPage {
    pub models: Vec<CatalogModel>,
    pub next_cursor: Option<String>,
}
/// Resolve URLs according to the selected wire, never by trying another protocol.
pub fn endpoint(provider: &Provider) -> Result<reqwest::Url> {
    if let Some(url) = &provider.catalog_url {
        return Ok(reqwest::Url::parse(url)?);
    }
    let base = provider.connection.endpoint.trim_end_matches('/');
    Ok(reqwest::Url::parse(&match provider.connection.protocol {
        Protocol::Gemini => format!("{base}/v1beta/models"),
        Protocol::Anthropic => format!("{base}/v1/models"),
        Protocol::Ollama => format!("{base}/api/tags"),
        Protocol::AzureOpenai => bail!(
            "azure_deployment_catalog_requires_explicit_catalog_url; enter deployment manually"
        ),
        _ => format!("{base}/models"),
    })?)
}
pub async fn fetch(provider: &Provider, secret: &str, cursor: Option<&str>) -> Result<CatalogPage> {
    ensure!(provider.enabled, "provider_disabled");
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut url = endpoint(provider)?;
    ensure!(
        ["http", "https"].contains(&url.scheme())
            && url.username().is_empty()
            && url.password().is_none(),
        "invalid_catalog_url"
    );
    if let Some(cursor) = cursor {
        ensure!(cursor.len() <= 2048, "catalog_cursor_limit");
        let key = match provider.connection.protocol {
            Protocol::Gemini => "pageToken",
            Protocol::Anthropic => "after_id",
            _ => "after",
        };
        url.query_pairs_mut().append_pair(key, cursor);
    }
    if provider.connection.protocol == Protocol::AzureOpenai {
        url.query_pairs_mut().append_pair(
            "api-version",
            provider
                .connection
                .api_version
                .as_deref()
                .context("api_version_required")?,
        );
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(15))
        .build()?;
    let mut request = client.get(url);
    match provider.connection.protocol {
        Protocol::Anthropic => {
            request = request.header("anthropic-version", "2023-06-01");
            if !secret.is_empty() {
                request = request.header("x-api-key", secret);
            }
        }
        Protocol::Gemini => {
            if !secret.is_empty() {
                request = request.header("x-goog-api-key", secret);
            }
        }
        Protocol::AzureOpenai => {
            if !secret.is_empty() {
                request = request.header("api-key", secret);
            }
        }
        _ => {
            if !secret.is_empty() {
                request = request.bearer_auth(secret);
            }
        }
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("catalog_network_error"))?;
    match response.status().as_u16() {
        401 | 403 => bail!("catalog_authentication_failed"),
        404 | 405 => bail!("catalog_not_available"),
        300..=399 => bail!("catalog_redirect_refused"),
        200..=299 => {}
        n => bail!("catalog_http_error_{n}"),
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 1024 * 1024,
            "catalog_response_limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    decode(
        &provider.connection.protocol,
        &serde_json::from_slice(&bytes).context("invalid_catalog_response")?,
    )
}
fn decode(protocol: &Protocol, body: &Value) -> Result<CatalogPage> {
    let entries = if *protocol == Protocol::Gemini || *protocol == Protocol::Ollama {
        body.get("models")
    } else {
        body.get("data").or_else(|| body.get("value"))
    }
    .and_then(Value::as_array)
    .context("invalid_catalog_models")?;
    ensure!(entries.len() <= 1000, "catalog_entry_limit");
    let mut models = Vec::new();
    for entry in entries {
        let id = match protocol {
            Protocol::Gemini | Protocol::Ollama => entry["name"].as_str(),
            _ => entry["id"].as_str(),
        }
        .context("catalog_model_id_missing")?;
        ensure!(
            !id.is_empty() && id.len() <= 256,
            "invalid_catalog_model_id"
        );
        let id = if *protocol == Protocol::Gemini {
            id.strip_prefix("models/").unwrap_or(id)
        } else {
            id
        };
        let mut capabilities = Capabilities::default();
        if let Some(reported) = entry.get("capabilities") {
            capabilities = serde_json::from_value(reported.clone()).unwrap_or_default();
        }
        capabilities.source = Some("provider_catalog".into());
        models.push(CatalogModel {
            id: id.into(),
            name: entry
                .get("displayName")
                .or_else(|| entry.get("display_name"))
                .or_else(|| entry.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(id)
                .into(),
            context_window: entry
                .get("inputTokenLimit")
                .or_else(|| entry.get("context_window"))
                .or_else(|| entry.get("context_length"))
                .and_then(Value::as_u64),
            max_output_tokens: entry
                .get("outputTokenLimit")
                .or_else(|| entry.get("max_output_tokens"))
                .and_then(Value::as_u64),
            capabilities,
        });
    }
    let next_cursor = body
        .get("nextPageToken")
        .or_else(|| body.get("next_cursor"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            if body["has_more"] == true {
                body["last_id"].as_str().map(str::to_owned)
            } else {
                None
            }
        });
    Ok(CatalogPage {
        models,
        next_cursor,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn metadata_remains_unknown_instead_of_inferred_from_names() {
        let p = decode(
            &Protocol::OpenaiChat,
            &json!({"data":[{"id":"reasoning-latest"}]}),
        )
        .unwrap();
        assert!(p.models[0].capabilities.reasoning_levels.is_empty());
        assert_eq!(p.models[0].capabilities.tools, None);
        let p=decode(&Protocol::Gemini,&json!({"models":[{"name":"models/example","inputTokenLimit":32000,"outputTokenLimit":4096}]})).unwrap();
        assert_eq!(p.models[0].id, "example");
        assert_eq!(p.models[0].context_window, Some(32000));
    }
}
