//! Actor-maintained PTY input boundary. A screen or hook cannot manufacture this state.
use super::*;
use serde_json::{Value, json};

fn nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_nanos()).ok())
        .unwrap_or(u64::MAX)
}
fn boundary(info: &SessionInfo) -> Value {
    serde_json::from_str::<Value>(&info.shell_status).unwrap_or(Value::Null)["host_input_boundary"]
        .clone()
}
pub(super) fn initialize(info: &mut SessionInfo) {
    info.shell_status=json!({"host_input_boundary":{"input_revision":0,"last_input_at_ns":nanos(),"input_buffer_empty":false}}).to_string();
}
pub(super) fn input(info: &mut SessionInfo) {
    let mut shell: Value = serde_json::from_str(&info.shell_status).unwrap_or_else(|_| json!({}));
    let revision = shell["host_input_boundary"]["input_revision"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(1);
    shell["host_input_boundary"] =
        json!({"input_revision":revision,"last_input_at_ns":nanos(),"input_buffer_empty":false});
    info.shell_status = shell.to_string();
}
pub(super) fn observe(info: &mut SessionInfo, observation: Option<Value>) {
    let mut boundary = boundary(info);
    let mut shell = observation.unwrap_or_else(|| json!({}));
    let prompt = shell["reported_at_ns"].as_u64();
    let last = boundary["last_input_at_ns"].as_u64();
    boundary["input_buffer_empty"] = json!(
        shell["phase"] == "prompt"
            && prompt
                .zip(last)
                .is_some_and(|(prompt, last)| prompt > last && prompt <= nanos())
            && crate::process::shell_foreground(info)
    );
    shell["host_input_boundary"] = boundary;
    info.shell_status = shell.to_string();
}
pub(super) fn commit(
    permit: &ai_terminal_agent_runtime::host::AuthorizationPermit,
    info: &SessionInfo,
    revision: u64,
) -> Result<()> {
    let shell: Value = serde_json::from_str(&info.shell_status).unwrap_or(Value::Null);
    let boundary = &shell["host_input_boundary"];
    let empty = shell["phase"] == "prompt"
        && shell["reported_at_ns"]
            .as_u64()
            .zip(boundary["last_input_at_ns"].as_u64())
            .is_some_and(|(prompt, last)| prompt > last && prompt <= nanos())
        && crate::process::shell_foreground(info);
    let observation = json!({"cwd":crate::process::cwd(info).map(|p|p.to_string_lossy().into_owned()),"revision":revision,"input_buffer_empty":empty,"phase":shell["phase"]});
    permit.commit(Some(&observation))
}
