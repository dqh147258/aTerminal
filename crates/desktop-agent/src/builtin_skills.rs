pub(crate) const CATALOG: &[(&str, &str, &str)] = &[
    (
        "builtin/terminal-visual",
        "Capture and inspect the authoritative terminal grid",
        "Use skill_action with skill_id=builtin/terminal-visual, action=capture, arguments={session_id?}. It returns a rendered_terminal PNG at one epoch/revision, not an OS desktop screenshot. A text-only model receives associated text and must not claim to see the image.",
    ),
    (
        "builtin/session-lifecycle",
        "Create, close or resize terminal sessions",
        "Use skill_action with skill_id=builtin/session-lifecycle and action=create|close|resize. create arguments are cwd, command array and optional shell_integration; resize requires rows/cols. Close terminates the process but preserves memory. Writes require a current grant and Desktop attachment.",
    ),
    (
        "builtin/agent-control",
        "Delegate work or stop session agents",
        "Global agents use send_agent_message with session_id and message. It returns a task_id immediately. get_agent_state reads progress; reports never wake a completed root. Stop uses skill_action skill_id=builtin/agent-control action=stop arguments={session_id}. Stop does not send Ctrl-C.",
    ),
    (
        "builtin/wait-terminal",
        "Wait for terminal changes or delay before reading completion evidence",
        "Terminal tasks can take time. Use wait with explicit duration_ms (integer 1–30000) for a pure delay; it returns actual elapsed_ms, never reads or changes a Terminal, and never proves completion. Then call get_terminal_state/read_terminal; if unfinished, repeat wait and read until reliable completion evidence, cancellation or the run time budget is exhausted. Never infer completion from quiet output or a prompt. The existing skill_action skill_id=builtin/wait-terminal action=wait arguments={session_id?,timeout_ms} instead waits for a terminal revision change or timeout and returns changed/state; its behavior is preserved. Both waits are bounded inside this user run. A shell still running does not mean its command is running; an application task stays unknown without an adapter.",
    ),
    (
        "builtin/terminal-history",
        "Locate earlier terminal evidence with raw content anchors",
        "Read tail with max_lines=200. To extend upward, use search with start_before={record_id,edge:head}, the previous view_id and max_lines=1000. An older filtered search tail can be stop_before. Raw display edges default to head=10/tail=20 and are configurable; they are not automatically safe search anchors. Logs may end in a dynamic TUI: remove status bars, progress/spinner rows, input prompts and UI borders from every anchor. Analysis labels exact tui_lines; Host validates them and derives search_head_anchor/search_tail_anchor without changing raw evidence. Prefer record edge references; explicit lines/candidates may also pass tui_lines. If only TUI remains, use screen or read_record rather than an empty search. Both bounds are excluded. Missing/ambiguous starts must not fall back to tail. Expired views can only be recovered from archived UUID records.",
    ),
];

pub(crate) fn mirror(root: &std::path::Path) -> anyhow::Result<()> {
    use std::io::Write;
    let directory = root.join("builtin");
    crate::service::secure_dir(&directory)?;
    let mut manifest = Vec::new();
    for (id, description, body) in CATALOG {
        let name = id.strip_prefix("builtin/").unwrap();
        let path = directory.join("skills").join(name);
        crate::service::secure_dir(&path)?;
        let text = format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n");
        let mut file = crate::service::open_private(&path.join("SKILL.md"), false)?;
        file.set_len(0)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        manifest.push(serde_json::json!({"id":id,"builtin":true,"read_only":true,"hash":blake3::hash(text.as_bytes()).to_hex().to_string()}));
    }
    let mut file = crate::service::open_private(&directory.join("manifest.json"), false)?;
    file.set_len(0)?;
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"mcp":["builtin/terminal"],"skills":manifest}),
    )?;
    file.sync_all()?;
    Ok(())
}
