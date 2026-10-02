//! Actor-owned PTY input boundary. Hook mtimes cannot prove that queued/typeahead input is empty.
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
    info.shell_status=json!({"host_input_boundary":{"input_revision":0,"last_input_at_ns":nanos(),"input_buffer_empty":false,"submission":null}}).to_string();
}
/// A prompt is clean only before any Actor input or after the exact complete
/// submission made from a previously clean prompt, with no intervening writes.
fn empty_evidence(boundary: &Value, shell: &Value) -> bool {
    if shell["phase"] != "prompt"
        || shell["instance"].as_str().is_none()
        || shell["sequence"].as_u64().is_none()
    {
        return false;
    }
    if boundary["input_revision"] == 0 {
        return shell["sequence"] == 0;
    }
    let submitted = &boundary["submission"];
    submitted.is_object()
        && submitted["input_revision"] == boundary["input_revision"]
        && submitted["instance"] == shell["instance"]
        && submitted["sequence"] == shell["sequence"]
        && submitted["command"] == shell["command"]
        && shell["command_association"] == true
        && shell["reported_at_ns"]
            .as_u64()
            .zip(boundary["last_input_at_ns"].as_u64())
            .is_some_and(|(prompt, last)| prompt > last && prompt <= nanos())
}
pub(super) fn input(info: &mut SessionInfo, command: Option<&str>) {
    let mut shell: Value = serde_json::from_str(&info.shell_status).unwrap_or_else(|_| json!({}));
    let old = boundary(info);
    let revision = old["input_revision"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(1);
    let submission = if let Some(command) = command.filter(|c| !c.contains(['\r', '\n']))
        && empty_evidence(&old, &shell)
        && crate::process::shell_foreground(info)
        && shell["command_association"] == true
        && let Some(sequence) = shell["sequence"].as_u64().and_then(|s| s.checked_add(1))
    {
        json!({"input_revision":revision,"instance":shell["instance"],"sequence":sequence,"command":command})
    } else {
        Value::Null
    };
    shell["host_input_boundary"] = json!({"input_revision":revision,"last_input_at_ns":nanos(),"input_buffer_empty":false,"submission":submission});
    info.shell_status = shell.to_string();
}
pub(super) fn observe(info: &mut SessionInfo, observation: Option<Value>) {
    let mut boundary = boundary(info);
    let mut shell = observation.unwrap_or_else(|| json!({}));
    boundary["input_buffer_empty"] =
        json!(empty_evidence(&boundary, &shell) && crate::process::shell_foreground(info));
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
    let empty = empty_evidence(boundary, &shell) && crate::process::shell_foreground(info);
    let observation = json!({"cwd":crate::process::cwd(info).map(|p|p.to_string_lossy().into_owned()),"revision":revision,"input_buffer_empty":empty,"phase":shell["phase"],"fence":{"session_id":info.id,"session_epoch":info.epoch,"manual_revision":info.manual_revision,"input_revision":boundary["input_revision"]}});
    permit.commit(Some(&observation))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prompt(sequence: u64, command: &str) -> Value {
        json!({"phase":"prompt","instance":"shell-one","sequence":sequence,"command":command,"command_association":true,"reported_at_ns":nanos().saturating_sub(1)})
    }
    #[test]
    fn a_newer_prompt_never_clears_unknown_raw_input_or_typeahead() {
        let boundary = json!({"input_revision":1,"last_input_at_ns":1,"submission":null});
        assert!(!empty_evidence(&boundary, &prompt(1, "command")));
        let boundary = json!({"input_revision":3,"last_input_at_ns":1,"submission":{"input_revision":2,"instance":"shell-one","sequence":1,"command":"/usr/bin/printf literal"}});
        assert!(!empty_evidence(
            &boundary,
            &prompt(1, "/usr/bin/printf literal")
        ));
    }
    #[test]
    fn initial_zero_input_prompt_and_exact_single_submission_are_provable() {
        assert!(empty_evidence(&json!({"input_revision":0}), &prompt(0, "")));
        assert!(!empty_evidence(
            &json!({"input_revision":0}),
            &prompt(2, "unrelated")
        ));
        let boundary = json!({"input_revision":1,"last_input_at_ns":1,"submission":{"input_revision":1,"instance":"shell-one","sequence":1,"command":"/usr/bin/printf literal"}});
        assert!(empty_evidence(
            &boundary,
            &prompt(1, "/usr/bin/printf literal")
        ));
        assert!(!empty_evidence(&boundary, &prompt(1, "different")));
        assert!(!empty_evidence(
            &boundary,
            &prompt(2, "/usr/bin/printf literal")
        ));
        let mut changed = prompt(1, "/usr/bin/printf literal");
        changed["instance"] = json!("different-shell");
        assert!(!empty_evidence(&boundary, &changed));
        let mut changed = prompt(1, "/usr/bin/printf literal");
        changed["command_association"] = json!(false);
        assert!(!empty_evidence(&boundary, &changed));
    }
}
