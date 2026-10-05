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
        "Global agents use send_agent_message with session_id and message. It returns a task_id (the child Run ID) immediately; additional messages to the same active child share it. Use get_agent_task with task_id for that exact task's state, done, result_text, result_record_id and error. Use wait_agent_task with task_id and explicit integer timeout_ms=1–30000 to wait for its outcome; timed_out=true leaves the child running, so repeat while needed within the shared user Run deadline. A stopped task can be completed, cancelled, paused, failed or orphaned; inspect state/error instead of assuming success. Agent completion does not prove terminal application success. Long retained results can be recovered with read_record using result_record_id. get_agent_state reads the Session's current/latest run, not a specific task. Reports never wake a completed root. Use list_agent_tasks for bounded discovery, get_agent_tasks for a fixed set of exact IDs, and wait_agent_tasks with task_ids, mode=any|all and timeout_ms=1–30000. cancel_agent_task cancels only that exact Run after authorization; an old task cannot cancel a newer Run in its Session. It does not send Ctrl-C. The legacy skill_action stop remains available for an explicitly authorized current Session stop.",
    ),
    (
        "builtin/wait-terminal",
        "Wait for terminal changes or delay before reading completion evidence",
        "Terminal tasks can take time. Use wait with explicit duration_ms (integer 1–30000) for a pure delay; it returns actual elapsed_ms, never reads or changes a Terminal, and never proves completion. Then call get_terminal_state/read_terminal; if unfinished, repeat wait and read until reliable completion evidence, cancellation or the run time budget is exhausted. Never infer completion from quiet output or a prompt. Prefer wait_terminal with after_revision from your last observed view and explicit timeout_ms=1–30000 (Global must also provide session_id). It returns updates that happened before waiting as changed=true, and requires a fresh read if the revision reset. Change does not establish completion. The existing skill_action skill_id=builtin/wait-terminal action=wait arguments={session_id?,timeout_ms} waits for a terminal revision change or timeout and returns changed/state; its behavior is preserved. Both waits are bounded inside this user run. A shell still running does not mean its command is running; an application task stays unknown without an adapter.",
    ),
    (
        "builtin/command-results",
        "Submit commands and collect correlated shell outcomes",
        "Use inspect_command for strictly supported read-only command syntax; it runs a fixed native program in the OS-observed Session cwd, with bounded stdout/stderr, explicit truncation and source=sidecar_read. It does not evaluate Shell aliases/functions or alter PTY input. Unsupported syntax is rejected. Use run_program for an absolute native program, literal args and optional UTF-8 stdin (at most 16000 bytes, default EOF; Global requires session_id). It uses direct exec/env_clear in the OS-observed cwd, never Shell/PTY input, and returns source=native_program with a persistent command_id, real child exit and bounded archived stdout/stderr. Only pinned native leaf programs can receive permanent rules; interpreters/wrappers/unknown programs require once/full. Native cancellation kills only its own process group. Use run_command with the complete command (Global also requires session_id); PTY dispatch cannot receive permanent rules because user functions/aliases may intercept even absolute paths. It uses the same PTY authorization, cancellation and manual-input fence as ordinary input; accepted only confirms submission. Collect its command_id with get_command_result or wait_command and explicit timeout_ms=1–30000. Completed requires the next exact shell sequence, matching full command and its prompt exit code, with a proven empty input boundary. Missing hooks, history transformations, existing DEBUG traps, manual/intervening input and sequence conflicts stay unknown. Results persist across Agent runs until retention removes them. Shell completion does not prove completion of background processes or arbitrary TUI/application tasks; those have no universal adapter. get_capabilities reports the actual role/tools, vision, observational shell hooks, permission state and shared remaining budget. ask_user routes a bounded question to the real user; its answer does not grant additional operation permissions.",
    ),
    (
        "builtin/terminal-history",
        "Locate earlier terminal evidence with raw content anchors",
        "Read tail with max_lines=200. To extend upward, use search with start_before={record_id,edge:head}, the previous view_id and max_lines=1000. An older filtered search tail can be stop_before. Raw display edges default to head=10/tail=20 and are configurable; they are not automatically safe search anchors. Logs may end in a dynamic TUI: remove status bars, progress/spinner rows, input prompts and UI borders from every anchor. Analysis labels exact tui_lines; Host validates them and derives search_head_anchor/search_tail_anchor without changing raw evidence. Prefer record edge references; explicit lines/candidates may also pass tui_lines. If only TUI remains, use screen or read_record rather than an empty search. Both bounds are excluded. Missing/ambiguous starts must not fall back to tail. Expired views can only be recovered from archived UUID records. read_terminal mode=delta requires after_revision and an observed view_id; grid/TUI changes return refetch_required rather than invented append logs. search_history searches retained Agent event/record text with optional Session/type/time filters, bounded snippets, exact event/record IDs and a generation-bound cursor. The full retained text is scanned in bounded chunks; an empty page with scan_incomplete=true and a cursor requires continuing the scan. It is separate from terminal content-anchor search.",
    ),
];

pub(crate) fn mirror(root: &std::path::Path) -> anyhow::Result<()> {
    use std::io::Write;
    let directory = root.join("builtin");
    crate::service::startup_stage("builtin:root-start");
    crate::service::secure_dir(&directory)?;
    crate::service::startup_stage("builtin:root-ready");
    let mut manifest = Vec::new();
    for (id, description, body) in CATALOG {
        let name = id.strip_prefix("builtin/").unwrap();
        let path = directory.join("skills").join(name);
        crate::service::startup_stage("builtin:skill-directory-start");
        crate::service::secure_dir(&path)?;
        crate::service::startup_stage("builtin:skill-directory-ready");
        let text = format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n");
        crate::service::startup_stage("builtin:skill-open-start");
        let mut file = crate::service::open_private(&path.join("SKILL.md"), false)?;
        crate::service::startup_stage("builtin:skill-open-ready");
        file.set_len(0)?;
        file.write_all(text.as_bytes())?;
        crate::service::startup_stage("builtin:skill-sync-start");
        file.sync_all()?;
        crate::service::startup_stage("builtin:skill-ready");
        manifest.push(serde_json::json!({"id":id,"builtin":true,"read_only":true,"hash":blake3::hash(text.as_bytes()).to_hex().to_string()}));
    }
    crate::service::startup_stage("builtin:manifest-open-start");
    let mut file = crate::service::open_private(&directory.join("manifest.json"), false)?;
    crate::service::startup_stage("builtin:manifest-open-ready");
    file.set_len(0)?;
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"mcp":["builtin/terminal"],"skills":manifest}),
    )?;
    crate::service::startup_stage("builtin:manifest-sync-start");
    file.sync_all()?;
    crate::service::startup_stage("builtin:manifest-ready");
    Ok(())
}
