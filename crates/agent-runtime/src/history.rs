//! Versioned project history. Rig objects exist only at the adapter boundary.
use crate::model::{ContextEntry, Origin, Protocol};
use anyhow::{Context, Result, bail, ensure};
use rig_core::message::{self as wire, AssistantContent, ToolResultContent, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub unit_id: String,
    pub origin: Origin,
    pub root_user_message_id: Option<String>,
    pub artifacts: Vec<String>,
    pub provider: Protocol,
    pub role: Role,
    pub provider_message_id: Option<String>,
    pub parts: Vec<Part>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallIdentity {
    pub call_id: String,
    pub item_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayBlock {
    Signed { signature: String, text: String },
    Encrypted { data: String },
    Redacted { data: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultPart {
    Text { text: String },
    Json { value: Value },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
        provider_fields: Option<Value>,
    },
    ToolCall {
        call_id: String,
        provider_id: Option<CallIdentity>,
        name: String,
        arguments: Value,
        signature: Option<String>,
        provider_fields: Option<Value>,
    },
    ToolResult {
        call_id: String,
        provider_id: Option<CallIdentity>,
        name: String,
        content: Vec<ResultPart>,
    },
    Replay {
        id: Option<String>,
        blocks: Vec<ReplayBlock>,
    },
    ImageRecord {
        record_id: String,
        #[serde(default)]
        media_type: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub schema_version: u32,
    pub generation: u64,
    pub covered_event_seq: i64,
    pub history_generation: i64,
    pub entries: Vec<Entry>,
    pub retained_facts: Vec<String>,
    pub archive_roots: Vec<String>,
}
impl Default for Projection {
    fn default() -> Self {
        Self {
            schema_version: 1,
            generation: 0,
            covered_event_seq: 0,
            history_generation: 0,
            entries: vec![],
            retained_facts: vec![],
            archive_roots: vec![],
        }
    }
}
fn identity(value: &Option<wire::ProviderCallId>) -> Option<CallIdentity> {
    value.as_ref().map(|v| CallIdentity {
        call_id: v.call_id.clone(),
        item_id: v.item_id.clone(),
    })
}
fn restore(value: &Option<CallIdentity>) -> Option<wire::ProviderCallId> {
    value.as_ref().and_then(|v| {
        wire::ProviderCallId::new(v.call_id.clone()).map(|id| {
            if let Some(item) = &v.item_id {
                id.with_item_id(item)
            } else {
                id
            }
        })
    })
}
fn text(value: &wire::Text) -> Part {
    Part::Text {
        text: value.text.clone(),
        provider_fields: value
            .additional_params
            .clone()
            .map(wire::AdditionalParams::into_value),
    }
}
fn wire_text(text: &str, extra: &Option<Value>, same: bool) -> wire::Text {
    wire::Text {
        text: text.into(),
        additional_params: if same {
            extra
                .as_ref()
                .and_then(Value::as_object)
                .cloned()
                .and_then(wire::AdditionalParams::new)
        } else {
            None
        },
    }
}
impl Entry {
    pub fn capture(entry: &ContextEntry, provider: &Protocol) -> Result<Self> {
        let mut parts = Vec::new();
        let mut image_records = entry.artifacts.iter();
        let (role, message_id) = match &entry.message {
            wire::Message::User { content } => {
                for item in content {
                    parts.push(match item {
                        UserContent::Text(t) => text(t),
                        UserContent::ToolResult(r) => Part::ToolResult {
                            call_id: r.call.as_str().into(),
                            provider_id: identity(&r.provider),
                            name: r.name.clone(),
                            content: r
                                .content
                                .iter()
                                .map(|c| match c {
                                    ToolResultContent::Text(t) => Ok(ResultPart::Text {
                                        text: t.text.clone(),
                                    }),
                                    ToolResultContent::Json { value } => Ok(ResultPart::Json {
                                        value: value.clone(),
                                    }),
                                    _ => bail!("tool images must use a separate image record"),
                                })
                                .collect::<Result<_>>()?,
                        },
                        UserContent::Image(image) => Part::ImageRecord {
                            media_type: image.media_type.as_ref().map(|m| {
                                serde_json::to_value(m)
                                    .unwrap()
                                    .as_str()
                                    .unwrap()
                                    .to_owned()
                            }),
                            record_id: image_records
                                .next()
                                .context("image_record_required")?
                                .clone(),
                        },
                        _ => bail!("unsupported_user_media"),
                    });
                }
                (Role::User, None)
            }
            wire::Message::Assistant { id, content } => {
                for item in content {
                    match item {
                        AssistantContent::Text(t) => parts.push(text(t)),
                        AssistantContent::ToolCall(c) => parts.push(Part::ToolCall {
                            call_id: c.id.as_str().into(),
                            provider_id: identity(&c.provider),
                            name: c.function.name.clone(),
                            arguments: c.function.arguments.clone(),
                            signature: c.signature.clone(),
                            provider_fields: c.additional_params.clone(),
                        }),
                        AssistantContent::Reasoning(r) => {
                            // Only opaque/signed replay dependencies survive a checkpoint.
                            // Unsigned thinking and reasoning summaries are not archived.
                            let blocks = r
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    wire::ReasoningContent::Text {
                                        text,
                                        signature: Some(signature),
                                    } => Some(ReplayBlock::Signed {
                                        text: text.clone(),
                                        signature: signature.clone(),
                                    }),
                                    wire::ReasoningContent::Encrypted(data) => {
                                        Some(ReplayBlock::Encrypted { data: data.clone() })
                                    }
                                    wire::ReasoningContent::Redacted { data } => {
                                        Some(ReplayBlock::Redacted { data: data.clone() })
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>();
                            if !blocks.is_empty() {
                                parts.push(Part::Replay {
                                    id: r.id.clone(),
                                    blocks,
                                });
                            }
                        }
                        _ => bail!("unsupported_assistant_media"),
                    }
                }
                (Role::Assistant, id.clone())
            }
            wire::Message::System { .. } => bail!("system_instructions_are_not_history"),
        };
        ensure!(!parts.is_empty(), "no_visible_model_response");
        Ok(Self {
            id: entry.id.clone(),
            unit_id: entry.unit_id.clone().unwrap_or_else(|| entry.id.clone()),
            origin: entry.origin.clone(),
            root_user_message_id: entry.root_user_message_id.clone(),
            artifacts: entry.artifacts.clone(),
            provider: provider.clone(),
            role,
            provider_message_id: message_id,
            parts,
        })
    }
    pub fn expand(
        &self,
        provider: &Protocol,
        mut image: impl FnMut(&str) -> Result<String>,
    ) -> Result<ContextEntry> {
        let same = provider == &self.provider;
        let message = match self.role {
            Role::User => {
                let mut content = Vec::new();
                for part in &self.parts {
                    content.push(match part {
                        Part::Text {
                            text,
                            provider_fields,
                        } => UserContent::Text(wire_text(text, provider_fields, same)),
                        Part::ToolResult {
                            call_id,
                            provider_id,
                            name,
                            content,
                        } => UserContent::tool_result_for(
                            wire::ToolCallId::new(call_id.clone()).context("invalid_tool_id")?,
                            if same { restore(provider_id) } else { None },
                            name,
                            content
                                .iter()
                                .map(|p| match p {
                                    ResultPart::Text { text } => ToolResultContent::text(text),
                                    ResultPart::Json { value } => ToolResultContent::Json {
                                        value: value.clone(),
                                    },
                                })
                                .collect(),
                        ),
                        Part::ImageRecord {
                            record_id,
                            media_type,
                        } => UserContent::image_base64(
                            image(record_id)?,
                            Some(
                                media_type
                                    .as_ref()
                                    .map(|m| serde_json::from_value(serde_json::json!(m)))
                                    .transpose()?
                                    .unwrap_or(wire::ImageMediaType::PNG),
                            ),
                            None,
                        ),
                        _ => bail!("invalid_user_history_part"),
                    });
                }
                wire::Message::User { content }
            }
            Role::Assistant => {
                let mut content = Vec::new();
                for part in &self.parts {
                    match part {
                        Part::Text {
                            text,
                            provider_fields,
                        } => content.push(AssistantContent::Text(wire_text(
                            text,
                            provider_fields,
                            same,
                        ))),
                        Part::ToolCall {
                            call_id,
                            provider_id,
                            name,
                            arguments,
                            signature,
                            provider_fields,
                        } => {
                            let mut call = wire::ToolCall::new(
                                wire::ToolCallId::new(call_id.clone())
                                    .context("invalid_tool_id")?,
                                wire::ToolFunction::new(name.clone(), arguments.clone()),
                            );
                            if same {
                                call.provider = restore(provider_id);
                                call.signature = signature.clone();
                                call.additional_params = provider_fields.clone();
                            }
                            content.push(AssistantContent::ToolCall(call));
                        }
                        Part::Replay { id, blocks } if same => {
                            content.push(AssistantContent::Reasoning(wire::Reasoning {
                                id: id.clone(),
                                content: blocks
                                    .iter()
                                    .map(|b| match b {
                                        ReplayBlock::Signed { signature, text } => {
                                            wire::ReasoningContent::Text {
                                                text: text.clone(),
                                                signature: Some(signature.clone()),
                                            }
                                        }
                                        ReplayBlock::Encrypted { data } => {
                                            wire::ReasoningContent::Encrypted(data.clone())
                                        }
                                        ReplayBlock::Redacted { data } => {
                                            wire::ReasoningContent::Redacted { data: data.clone() }
                                        }
                                    })
                                    .collect(),
                            }))
                        }
                        Part::Replay { .. } => {}
                        _ => bail!("invalid_assistant_history_part"),
                    }
                }
                ensure!(!content.is_empty(), "empty_provider_history");
                wire::Message::Assistant {
                    id: if same {
                        self.provider_message_id.clone()
                    } else {
                        None
                    },
                    content,
                }
            }
        };
        Ok(ContextEntry {
            id: self.id.clone(),
            unit_id: Some(self.unit_id.clone()),
            origin: self.origin.clone(),
            root_user_message_id: self.root_user_message_id.clone(),
            artifacts: self.artifacts.clone(),
            message,
        })
    }
}
impl Projection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.entries.len() <= 4096,
            "invalid_projection"
        );
        let mut pending = std::collections::BTreeSet::new();
        for entry in &self.entries {
            for part in &entry.parts {
                match part {
                    Part::ToolCall { call_id, .. } => {
                        ensure!(pending.insert(call_id), "duplicate_tool_call");
                    }
                    Part::ToolResult { call_id, .. } => {
                        ensure!(pending.remove(call_id), "unpaired_tool_result");
                    }
                    _ => {}
                }
            }
        }
        ensure!(pending.is_empty(), "pending_tool_calls");
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_call_identity_round_trips_without_framework_serialization() {
        let live = ContextEntry {
            unit_id: None,
            id: "entry".into(),
            origin: Origin::Assistant,
            root_user_message_id: Some("user".into()),
            artifacts: vec![],
            message: wire::Message::Assistant {
                id: Some("msg_1".into()),
                content: vec![AssistantContent::tool_call_with_call_id(
                    "fc_1",
                    "call_1".into(),
                    "read_terminal",
                    serde_json::json!({}),
                )],
            },
        };
        let stored = Entry::capture(&live, &Protocol::OpenaiResponses).unwrap();
        let value = serde_json::to_value(&stored).unwrap();
        assert_eq!(value["parts"][0]["kind"], "tool_call");
        let restored = stored
            .expand(&Protocol::OpenaiResponses, |_| unreachable!())
            .unwrap();
        assert_eq!(
            serde_json::to_value(live.message).unwrap(),
            serde_json::to_value(restored.message).unwrap()
        );
        let foreign = stored
            .expand(&Protocol::Anthropic, |_| unreachable!())
            .unwrap();
        if let wire::Message::Assistant { id, content } = foreign.message {
            assert!(id.is_none());
            if let AssistantContent::ToolCall(c) = &content[0] {
                assert!(c.provider.is_none());
            }
        }
    }
}
