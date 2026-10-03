//! Opt-in deterministic model for real authorization RPC/PTY tests.
//! It emits tools, never resolves authorization or writes the command's marker itself.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Write, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    id: String,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    tool: String,
    arguments: Value,
}

fn text(message: &Value) -> String {
    message["content"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            message["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        })
}

fn scenario_text(message: &Value) -> Option<String> {
    let content = text(message);
    if let Some(prompt) = scenario_prompt(&content) {
        return Some(prompt);
    }
    // Host keeps the current user task separately from the lossy history
    // summary. Recover only that field, never a scenario quoted in old history.
    content.strip_prefix("Earlier context summary: ")?;
    let (_, archive) = content.rsplit_once("\nArchive UUID: ")?;
    let (_, constraints) = archive
        .split_once(". Original observations can be read by UUID. Current task constraints: ")?;
    let constraints: Vec<String> = serde_json::from_str(constraints).ok()?;
    constraints
        .iter()
        .rev()
        .find_map(|constraint| scenario_prompt(constraint))
}

fn scenario_prompt(content: &str) -> Option<String> {
    if content.starts_with("AUTH_REVIEW:") {
        return Some(content.to_owned());
    }
    let delegated = content.strip_prefix("Task delegated within the authenticated user run: ")?;
    let delegated: Value = serde_json::from_str(delegated).ok()?;
    let message = delegated["message"].as_str()?;
    (delegated["source"] == "delegated_task" && message.starts_with("AUTH_REVIEW:"))
        .then(|| message.to_owned())
}

// Later steps can consume exact IDs returned by earlier tools without asking
// the model to invent command/task IDs. The fixture never fabricates results.
fn arguments(value: &Value, scenario: &str, results: &[Value]) -> Result<Value> {
    if let Some(reference) = value.get("$fixture_ref") {
        ensure!(
            value.as_object().is_some_and(|map| map.len() == 1),
            "ambiguous fixture reference"
        );
        let step = reference["step"]
            .as_u64()
            .context("reference step required")?;
        let pointer = reference["pointer"]
            .as_str()
            .context("reference pointer required")?;
        let call = format!("auth-{scenario}-{step}");
        return results
            .iter()
            .rev()
            .filter(|result| result["call_id"] == call)
            .find_map(|result| result["result"].pointer(pointer))
            .cloned()
            .context("fixture reference result unavailable");
    }
    match value {
        Value::Array(items) => Ok(Value::Array(
            items
                .iter()
                .map(|item| arguments(item, scenario, results))
                .collect::<Result<_>>()?,
        )),
        Value::Object(items) => Ok(Value::Object(
            items
                .iter()
                .map(|(key, value)| Ok((key.clone(), arguments(value, scenario, results)?)))
                .collect::<Result<_>>()?,
        )),
        value => Ok(value.clone()),
    }
}

pub(super) fn respond(body: &Value, dir: &Path) -> Result<Option<axum::response::Response>> {
    let messages = body["messages"].as_array().context("messages required")?;
    // Compaction contains earlier scenario prompts, but is never an execution
    // request. Handle it before selecting a scenario so old steps cannot replay.
    if messages
        .last()
        .is_some_and(|message| text(message).starts_with("Application compression stage."))
    {
        return Ok(Some(super::fixture_sse(json!({"content":json!({
            "summary":"Earlier authorization fixture tasks are archived. Real execution and approval results remain in immutable records and independent marker files. Do not replay completed or uncertain actions; follow the current task only."
        }).to_string()}))));
    }
    let Some((start, prompt)) = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| message["role"] == "user")
        .find_map(|(index, message)| scenario_text(message).map(|text| (index, text)))
    else {
        return Ok(None);
    };
    let scenario: Scenario = serde_json::from_str(&prompt["AUTH_REVIEW:".len()..])?;
    ensure!(
        !scenario.id.is_empty()
            && scenario.id.len() <= 80
            && scenario
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
        "invalid authorization fixture scenario ID"
    );
    ensure!(scenario.steps.len() <= 24, "fixture step limit");
    let results = messages[start..]
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            let content = text(message);
            let provider=message["tool_call_id"].as_str().unwrap_or_default();
            let logical=provider.strip_suffix("-after-observe").unwrap_or(provider);
            json!({"call_id":logical,"provider_call_id":provider,"result":serde_json::from_str::<Value>(&content).unwrap_or(Value::String(content))})
        })
        .collect::<Vec<_>>();
    let analysis = messages
        .last()
        .is_some_and(|message| text(message).starts_with("Application analysis"));
    // Analysis can replace a raw tool result with a digest. Keep its exact IDs
    // for reference substitution, while advancement still follows this request's
    // tool-call IDs rather than a counter stored outside the conversation.
    let captured_path = dir.join(format!("authorization-results-{}.json", scenario.id));
    let mut captured: Vec<Value> = if captured_path.exists() {
        serde_json::from_slice(&std::fs::read(&captured_path)?)?
    } else {
        Vec::new()
    };
    for result in &results {
        if !captured.iter().any(|saved| saved == result) {
            captured.push(result.clone());
        }
    }
    ensure!(captured.len() <= 128, "fixture captured-result limit");
    std::fs::write(&captured_path, serde_json::to_vec(&captured)?)?;
    // Only fixture IDs/results are logged. Account credentials and request headers
    // stay in the private fixture config and are never copied into test reports.
    let observation = json!({"id":scenario.id,"analysis":analysis,"results":results});
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    writeln!(
        options.open(dir.join("authorization-model-observations.jsonl"))?,
        "{observation}"
    )?;
    if analysis {
        return Ok(Some(super::fixture_sse(json!({"content":json!({
            "summary":"Authorization fixture observation; execution is checked independently using PTY files and RPC state.",
            "key_quotes":[],"facts":[],"tui_lines":[],"open_questions":[]
        }).to_string()}))));
    }
    for (index, step) in scenario.steps.iter().enumerate() {
        let call_id = format!("auth-{}-{index}", scenario.id);
        let previous = results
            .iter()
            .rev()
            .find(|result| result["call_id"] == call_id);
        let mut provider_id = call_id.clone();
        if let Some(previous) = previous {
            if previous["result"]["error"] != "observe_terminal_after_uncertain_action" {
                continue;
            }
            ensure!(
                previous["provider_call_id"] == call_id,
                "Fixture observation did not clear the existing action barrier"
            );
            let observation_id = format!("{call_id}-observe");
            if let Some(observed) = results
                .iter()
                .rev()
                .find(|result| result["call_id"] == observation_id)
            {
                ensure!(
                    !observed["result"]["error"].is_string(),
                    "Fixture native observation failed"
                );
                provider_id = format!("{call_id}-after-observe");
            } else {
                let mut args = json!({"command":"pwd"});
                let global = body["tools"].as_array().is_some_and(|tools| {
                    tools.iter().any(|tool| {
                        tool["function"]["name"] == "inspect_command"
                            && tool["function"]["parameters"]["required"]
                                .as_array()
                                .is_some_and(|required| {
                                    required.iter().any(|field| field == "session_id")
                                })
                    })
                });
                if global {
                    let config: Value =
                        serde_json::from_slice(&std::fs::read(dir.join("account-fixture.json"))?)?;
                    args["session_id"] = step
                        .arguments
                        .get("session_id")
                        .cloned()
                        .unwrap_or_else(|| config["session"].clone());
                }
                return Ok(Some(super::fixture_sse(
                    json!({"role":"assistant","tool_calls":[{"index":0,"id":observation_id,"type":"function","function":{"name":"inspect_command","arguments":args.to_string()}}]}),
                )));
            }
        }
        ensure!(
            body["tools"].as_array().is_some_and(|tools| tools
                .iter()
                .any(|tool| tool["function"]["name"] == step.tool)),
            "fixture tool unavailable: {}",
            step.tool
        );
        return Ok(Some(super::fixture_sse(json!({
            "role":"assistant","tool_calls":[{"index":0,"id":provider_id,"type":"function",
            "function":{"name":step.tool,"arguments":arguments(&step.arguments, &scenario.id, &captured)?.to_string()}}]
        }))));
    }
    Ok(Some(super::fixture_sse(
        json!({"content":format!("AUTH_REVIEW_DONE:{}", scenario.id)}),
    )))
}
