//! Added Broker routes. All writes use the parent's fenced, guarded PTY channel.
use super::*;

fn check(context: &ToolContext) -> Result<()> {
    context.budget.remaining()?;
    ensure!(!*context.cancel.borrow(), "cancelled");
    Ok(())
}
fn shell(info: &SessionInfo) -> Value {
    serde_json::from_str(&info.shell_status).unwrap_or(Value::Null)
}
fn unknown(value: &mut Value, reason: &str) {
    value["state"] = json!("unknown");
    value["reason"] = json!(reason);
    value["exit_code"] = Value::Null;
    value["final"] = json!(true);
}
/// Pure correlation, deliberately excluding application-task completion.
fn correlate(value: &mut Value, info: &SessionInfo) {
    if value["final"] == true {
        return;
    }
    let observed = shell(info);
    let baseline = &value["baseline"];
    if value["epoch"] != info.epoch || value["manual_revision"] != info.manual_revision {
        unknown(value, "session_changed_or_manual_input");
        return;
    }
    if info.exited {
        unknown(value, "shell_exited_without_correlated_prompt");
        return;
    }
    if baseline["command_association"] != true
        || baseline["phase"] != "prompt"
        || baseline["sequence"].as_u64().is_none()
        || baseline["instance"].is_null()
    {
        unknown(value, "shell_submission_boundary_unknown");
        return;
    }
    if observed["instance"] != baseline["instance"] {
        unknown(value, "shell_hook_instance_changed_or_unavailable");
        return;
    }
    let Some(sequence) = observed["sequence"].as_u64() else {
        unknown(value, "shell_sequence_unavailable");
        return;
    };
    let expected = baseline["sequence"].as_u64().unwrap().checked_add(1);
    if Some(sequence) == expected {
        if observed["command"] != value["command"] {
            unknown(value, "shell_command_mismatch");
            return;
        }
        value["shell_evidence"] = observed.clone();
        value["cwd"] = observed["cwd"].clone();
        value["sequence"] = json!(sequence);
        if observed["phase"] == "prompt" {
            if observed["exit_code"].as_i64().is_none() {
                unknown(value, "shell_exit_code_unavailable");
                return;
            }
            value["state"] = json!("completed");
            value["exit_code"] = observed["exit_code"].clone();
            value["final"] = json!(true);
        } else {
            value["state"] = json!("running");
        }
    } else if sequence != baseline["sequence"].as_u64().unwrap() {
        unknown(value, "shell_sequence_conflict_or_evidence_expired");
    }
}
impl Backend {
    /// Parent calls this before its legacy match; None means this helper does not own the route.
    pub(super) async fn invoke_added(
        &self,
        context: &ToolContext,
        name: &str,
        args: Value,
    ) -> Result<Option<ToolOutput>> {
        check(context)?;
        let result = match name {
            "inspect_command" => self.inspect_command(context, args).await?,
            "run_command" => self.run_command(context, args)?,
            "get_command_result" => self.get_command_result(context, args)?,
            "wait_command" => self.wait_command(context, args).await?,
            "list_agent_tasks" => self.host()?.agents.list_agent_tasks(context, args)?,
            "get_agent_tasks" => self.host()?.agents.get_agent_tasks(context, args)?,
            "wait_agent_tasks" => self.host()?.agents.wait_agent_tasks(context, args).await?,
            "cancel_agent_task" => self.host()?.agents.cancel_agent_task(context, args)?,
            "search_history" => self.host()?.agents.search_history(context, args)?,
            "wait_terminal" => self.wait_terminal(context, args).await?,
            "read_terminal" if args["mode"] == "delta" => self.read_delta(context, args)?,
            "get_capabilities" => self.get_capabilities(context, args)?,
            _ => return Ok(None),
        };
        check(context)?;
        Ok(Some(result))
    }
    async fn inspect_command(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let session = self.session(&args)?;
        let command = args["command"].as_str().context("command_required")?;
        self.authorize(false).await?;
        check(context)?;
        let info = self.info(&session)?.info.context("session_unavailable")?;
        let cwd = crate::process::cwd(&info).context("current_session_cwd_unavailable")?;
        let plan = ai_terminal_agent_runtime::authorization::inspect_command_plan(command)
            .context("inspect_command_not_readonly; use run_command to request authorization")?;
        check(context)?;
        let inspection_context = self.inspection_context(context, &session, &cwd)?;
        let result = crate::process::inspect(
            &inspection_context,
            std::path::Path::new(&plan.program),
            &plan.args,
            &cwd,
        )
        .await?;
        self.authorize(false).await?;
        let stdout = encoded_stream(&result.stdout);
        let stderr = encoded_stream(&result.stderr);
        let body = serde_json::to_string(
            &json!({"stdout":stdout,"stderr":stderr,"exit_code":result.exit_code,"stdout_truncated":result.stdout_truncated,"stderr_truncated":result.stderr_truncated}),
        )?;
        let metadata = json!({"source":"sidecar_read","session_id":session,"program":plan.program,"argv":plan.args,"cwd":cwd,"exit_code":result.exit_code,"stdout_truncated":result.stdout_truncated,"stderr_truncated":result.stderr_truncated,"elapsed_ms":result.elapsed_ms,"pty_unchanged":true});
        Ok(ToolOutput {
            value: metadata.clone(),
            observation: Some(Observation {
                kind: "sidecar_read".into(),
                metadata,
                body: body.clone(),
                model_body: Some(body),
                binary: false,
                record_id: None,
            }),
            outcome: None,
        })
    }
    fn run_command(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let id = self.session(&args)?;
        let command = args["command"].as_str().context("command_required")?;
        ensure!(
            !command.trim().is_empty()
                && command.len() <= 16000
                && command
                    .chars()
                    .all(|c| !c.is_control() || matches!(c, '\n' | '\t')),
            "invalid_command"
        );
        let reply = self.info(&id)?;
        let info = reply.info.context("session_unavailable")?;
        let frame = reply.snapshot.context("snapshot_unavailable")?;
        ensure!(
            !command.contains('\n') || frame.input_modes & 2 != 0,
            "multiline_requires_bracketed_paste"
        );
        let mut value = json!({"state":"submitted","accepted":false,"final":false,"command":command,"baseline":shell(&info),"epoch":info.epoch,"manual_revision":info.manual_revision,"submitted_at":now(),"exit_code":null});
        // The actor's host_input_boundary is independent of shell text. Without it we can
        // still submit an approved command, but cannot claim a clean command association.
        if value["baseline"]["host_input_boundary"]["input_buffer_empty"] != true {
            value["association_unavailable_reason"] = json!("input_buffer_boundary_unknown");
        } else if value["baseline"]["command_association"] != true {
            value["association_unavailable_reason"] =
                json!("shell_command_association_unavailable");
        }
        let host = self.host()?;
        let command_id = host.agents.store.begin_command(
            &self.scope,
            &context.run_id,
            &context.action_id,
            &id,
            value,
        )?;
        let result = self.write(
            context,
            &id,
            Request {
                operation: Operation::AgentWrite as i32,
                input_kind: 1,
                text: command.into(),
                submit: true,
                ..Default::default()
            },
        );
        let mut saved = host.agents.store.command(&self.scope, &command_id)?;
        match result {
            Ok(mut output) => {
                saved["accepted"] = json!(output.value["accepted"] == true);
                if let Some(reason) = saved["association_unavailable_reason"]
                    .as_str()
                    .map(str::to_owned)
                {
                    unknown(&mut saved, &reason);
                }
                if output.value["accepted"] != true {
                    unknown(&mut saved, "input_not_accepted");
                }
                host.agents.store.set_command_acceptance(
                    &self.scope,
                    &command_id,
                    output.value["accepted"] == true,
                )?;
                host.agents
                    .store
                    .update_command(&self.scope, &command_id, &saved)?;
                output.value["command_id"] = json!(command_id);
                output.value["state"] = json!("submitted");
                output.value["completion"] = json!("unknown");
                Ok(output)
            }
            Err(error) => {
                unknown(&mut saved, "submission_outcome_unknown");
                host.agents
                    .store
                    .update_command(&self.scope, &command_id, &saved)?;
                Err(error)
            }
        }
    }
    fn command_result(&self, context: &ToolContext, command_id: &str) -> Result<Value> {
        check(context)?;
        let host = self.host()?;
        let mut value = host.agents.store.command(&self.scope, command_id)?;
        if value["final"] != true {
            let session = value["session_id"]
                .as_str()
                .context("command_session_missing")?;
            // Actor lookup rechecks current account ownership even when reading a historical ID.
            match self
                .info(session)
                .and_then(|r| r.info.context("session_unavailable"))
            {
                Ok(info) => correlate(&mut value, &info),
                Err(_) => unknown(&mut value, "session_unavailable"),
            }
            host.agents
                .store
                .update_command(&self.scope, command_id, &value)?;
            value = host.agents.store.command(&self.scope, command_id)?;
        }
        value.as_object_mut().unwrap().remove("baseline");
        Ok(value)
    }
    fn get_command_result(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let id = args["command_id"].as_str().context("command_id_required")?;
        let value = self.command_result(context, id)?;
        context.budget.read(serde_json::to_vec(&value)?.len())?;
        Ok(ToolOutput::value(value))
    }
    async fn wait_command(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let id = args["command_id"].as_str().context("command_id_required")?;
        let timeout = args["timeout_ms"].as_u64().context("timeout_ms_required")?;
        ensure!((1..=30000).contains(&timeout), "invalid_timeout_ms");
        let start = Instant::now();
        let end = start + Duration::from_millis(timeout);
        loop {
            let mut value = self.command_result(context, id)?;
            if value["final"] == true || Instant::now() >= end {
                value["timed_out"] = json!(value["final"] != true);
                value["elapsed_ms"] = json!(start.elapsed().as_millis() as u64);
                context.budget.read(serde_json::to_vec(&value)?.len())?;
                return Ok(ToolOutput::value(value));
            }
            pause(context, end).await?;
        }
    }
    async fn wait_terminal(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let id = self.session(&args)?;
        let after = args["after_revision"]
            .as_u64()
            .context("after_revision_required")?;
        let timeout = args["timeout_ms"].as_u64().context("timeout_ms_required")?;
        ensure!((1..=30000).contains(&timeout), "invalid_timeout_ms");
        let start = Instant::now();
        let end = start + Duration::from_millis(timeout);
        loop {
            check(context)?;
            let reply = self.info(&id)?;
            let frame = reply.snapshot.context("snapshot_unavailable")?;
            let changed = frame.revision > after;
            if changed || frame.revision < after || Instant::now() >= end {
                return Ok(ToolOutput::value(
                    json!({"session_id":id,"revision":frame.revision,"epoch":frame.epoch,"changed":changed,"refetch_required":frame.revision<after,"timed_out":!changed && frame.revision>=after,"elapsed_ms":start.elapsed().as_millis() as u64,"completion":"unknown"}),
                ));
            }
            pause(context, end).await?;
        }
    }
    fn get_capabilities(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let global = self.scope.session.is_none();
        let terminal = if !global || args.get("session_id").is_some() {
            let session = self.session(&args)?;
            let info = self.info(&session)?.info.context("session_unavailable")?;
            let hooks = shell(&info);
            json!({"session_id":session,"desktop_attached":info.desktop_attached,"shell_foreground_proven":crate::process::shell_foreground(&info),"shell_hooks":{"available":hooks["evidence_source"]=="session_shell_hook","dialect":hooks["dialect"],"command_association":hooks["command_association"]==true,"evidence_source":hooks["evidence_source"],"trusted_for_authorization":false},"application_task_adapter":null,"application_completion":false})
        } else {
            Value::Null
        };
        Ok(ToolOutput::value(
            json!({"role":if global{"global"}else{"session"},"tools":terminal_tools(global).iter().map(|t|t.name.as_str()).collect::<Vec<_>>(),"vision":context.vision,"visual":{"capture_source":"rendered_terminal","model_can_see_images":context.vision},"native_inspection":{"os_cwd_available":cfg!(unix),"source":"sidecar_read","max_elapsed_ms":30000},"terminal":terminal,"authorization":self.host()?.agents.permission_capabilities(context)?,"budget":context.budget.tool_status()?,"limits":{"wait_ms":30000,"task_ids":32,"history_page":50,"shell_evidence":"observational","os_sandbox":false,"cwd_sandbox":false,"mcp_catalog_may_start_enabled_servers":true}}),
        ))
    }
    fn read_delta(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let session = self.session(&args)?;
        let after = args["after_revision"]
            .as_u64()
            .context("after_revision_required")?;
        let view_id = args["view_id"].as_str();
        let previous = {
            let cache = self.views.lock().unwrap();
            cache
                .views
                .iter()
                .find(|v| {
                    Some(v.id.as_str()) == view_id
                        && v.session == session
                        && v.at.elapsed() < Duration::from_secs(600)
                })
                .map(|v| v.view.clone())
        };
        let reply = request_actor(
            &self.actor(&session)?,
            Request {
                operation: Operation::ObserveTerminal as i32,
                client: self.client,
                session: session.clone(),
                ..Default::default()
            },
        )?;
        ensure!(reply.error.is_empty(), "{}", reply.error);
        let current: ReadView =
            serde_json::from_str(reply.history.first().context("read_view_unavailable")?)?;
        let (state, reason) = delta_state(previous.as_deref(), &current, after);
        // A mutable grid lacks an append-only provenance guarantee. Changes require an
        // authoritative fresh read; never splice screen rows into a fabricated log.
        Ok(ToolOutput::value(
            json!({"session_id":session,"after_revision":after,"revision":current.revision,"epoch":current.epoch,"state":state,"reason":reason,"refetch_required":state=="refetch_required","next_read":if state=="refetch_required"{json!({"mode":"tail"})}else{Value::Null},"text":"","completion":"unknown","max_read_bytes":context.max_read_bytes}),
        ))
    }
}
fn encoded_stream(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => json!({"encoding":"utf8","text":text}),
        Err(_) => json!({"encoding":"base64","data":STANDARD.encode(bytes)}),
    }
}
fn delta_state(
    previous: Option<&ReadView>,
    current: &ReadView,
    after: u64,
) -> (&'static str, &'static str) {
    let Some(previous) = previous else {
        return ("refetch_required", "view_expired_or_missing");
    };
    if previous.revision != after
        || current.epoch != previous.epoch
        || current.dimensions_epoch != previous.dimensions_epoch
        || current.revision < after
    {
        return ("refetch_required", "view_revision_or_dimensions_changed");
    }
    if current.revision == after {
        return ("unchanged", "same_revision");
    }
    if previous.alternate_screen || current.alternate_screen {
        return ("refetch_required", "mutable_tui_screen");
    }
    (
        "refetch_required",
        "screen_changed_without_append_only_provenance",
    )
}
async fn pause(context: &ToolContext, end: Instant) -> Result<()> {
    let mut cancel = context.cancel.clone();
    let remaining = context.budget.remaining()?;
    tokio::select! {biased; _=cancel.wait_for(|v|*v)=>bail!("cancelled"), _=tokio::time::sleep(remaining.min(Duration::from_millis(50)).min(end.saturating_duration_since(Instant::now())))=>{}}
    check(context)
}

#[cfg(test)]
mod toolset_tests {
    use super::*;
    fn info(phase: &str, sequence: u64, command: &str, exit: Option<i32>) -> SessionInfo {
        SessionInfo{epoch:1,manual_revision:2,shell_status:json!({"phase":phase,"sequence":sequence,"command":command,"exit_code":exit,"instance":"hook","cwd":"/changed"}).to_string(),..Default::default()}
    }
    fn submitted() -> Value {
        json!({"command":"false","epoch":1,"manual_revision":2,"state":"submitted","final":false,"baseline":{"phase":"prompt","sequence":3,"instance":"hook","command_association":true},"application_task":{"state":"unknown"}})
    }
    #[test]
    fn completion_requires_exact_sequence_command_and_prompt_exit() {
        let mut value = submitted();
        correlate(&mut value, &info("prompt", 3, "previous", Some(0)));
        assert_eq!(value["state"], "submitted");
        assert!(value["exit_code"].is_null());
        correlate(&mut value, &info("running", 4, "false", None));
        assert_eq!(value["state"], "running");
        assert!(value["exit_code"].is_null());
        correlate(&mut value, &info("prompt", 4, "false", Some(1)));
        assert_eq!(value["state"], "completed");
        assert_eq!(value["exit_code"], 1);
        assert_eq!(value["cwd"], "/changed");
        assert_eq!(value["application_task"]["state"], "unknown");
    }
    #[test]
    fn conflicts_manual_input_and_missing_codes_stay_unknown() {
        for evidence in [
            info("prompt", 4, "other", Some(0)),
            info("prompt", 5, "false", Some(0)),
            info("prompt", 4, "false", None),
            SessionInfo {
                manual_revision: 3,
                ..info("prompt", 4, "false", Some(0))
            },
        ] {
            let mut value = submitted();
            correlate(&mut value, &evidence);
            assert_eq!(value["state"], "unknown");
            assert!(value["exit_code"].is_null());
        }
        let mut value = submitted();
        value["baseline"] = Value::Null;
        correlate(&mut value, &info("prompt", 4, "false", Some(0)));
        assert_eq!(value["state"], "unknown");
    }
    #[test]
    fn delta_references_require_same_view_and_tui_changes_refetch() {
        let view = ReadView {
            epoch: 1,
            revision: 5,
            dimensions_epoch: 1,
            alternate_screen: false,
            screen_start: 0,
            source_partial: false,
            lines: vec![],
        };
        assert_eq!(delta_state(Some(&view), &view, 5).0, "unchanged");
        assert_eq!(delta_state(None, &view, 5).0, "refetch_required");
        assert_eq!(delta_state(Some(&view), &view, 4).0, "refetch_required");
        let changed = ReadView {
            revision: 6,
            alternate_screen: true,
            ..view.clone()
        };
        assert_eq!(
            delta_state(Some(&view), &changed, 5),
            ("refetch_required", "mutable_tui_screen")
        );
        let resized = ReadView {
            revision: 6,
            dimensions_epoch: 2,
            ..view.clone()
        };
        assert_eq!(delta_state(Some(&view), &resized, 5).0, "refetch_required");
    }
}

#[cfg(all(test, unix))]
mod toolset_broker_tests {
    use super::*;
    use ai_terminal_agent_runtime::{
        host::{AgentHost, Budget},
        model::{Connection, Protocol},
        store::Store,
    };
    use portable_pty::{PtySize, native_pty_system};
    use std::{io::Write, sync::atomic::AtomicU64};
    struct Fixture {
        backend: Arc<Backend>,
        context: ToolContext,
        cancel: tokio::sync::watch::Sender<bool>,
        host: Arc<Host>,
        revision: Arc<AtomicU64>,
        _temp: tempfile::TempDir,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let state = temp.path().join("state");
            secure_dir(&state).unwrap();
            let store = Arc::new(Store::open(&state.join("data/db")).unwrap());
            let scope = store.agent("owner", "desktop", Some("s")).unwrap();
            let root = store
                .accept_user(&scope, "r", "fixture", json!({}))
                .unwrap();
            let host = Arc::new(Host {
                agents: AgentHost::new(store, tokio::runtime::Handle::current()),
                state_dir: state.clone(),
                account: crate::account::AccountManager::new(&state).unwrap(),
                config: crate::config::ConfigService::open(&state).unwrap(),
                assistant: crate::assistant::Assistant::default(),
                sessions: Mutex::new(HashMap::new()),
                session_order: Mutex::new(vec!["s".into()]),
                recent_directories: Mutex::new(crate::recent_directories::RecentDirectories::new(
                    &state,
                )),
                owners: Mutex::new(HashMap::from([("s".into(), "owner".into())])),
                stop: Arc::new(AtomicBool::new(false)),
                workers: AtomicUsize::new(0),
            });
            let original = temp.path().join("shell-config");
            std::fs::create_dir(&original).unwrap();
            std::fs::write(
                original.join(".bashrc"),
                "PS1='fixture> '; HISTCONTROL=; HISTIGNORE=;\n",
            )
            .unwrap();
            let (integration, mut command) =
                crate::shell::Integration::prepare(temp.path(), &["/bin/bash".into()]).unwrap();
            command.env("HOME", &original);
            command.cwd(temp.path());
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let mut child = pair.slave.spawn_command(command).unwrap();
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader().unwrap();
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut reader, &mut std::io::sink());
            });
            let mut writer = pair.master.take_writer().unwrap();
            let start = Instant::now();
            while integration.observation().is_none() {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(10));
            }
            let (actor, requests) = mpsc::sync_channel::<ActorMessage>(16);
            let revision = Arc::new(AtomicU64::new(10));
            let observed = revision.clone();
            std::thread::spawn(move || {
                // This is a protocol-local actor stub around a real isolated PTY; no live
                // Desktop, account or model is contacted. Initial draft is known empty.
                for message in requests {
                    let mut observation = integration.observation().unwrap_or(Value::Null);
                    observation["host_input_boundary"] =
                        json!({"input_buffer_empty":observation["phase"]=="prompt"});
                    let info = SessionInfo {
                        id: "s".into(),
                        epoch: 1,
                        manual_revision: 2,
                        control_epoch: 3,
                        desktop_attached: true,
                        shell_status: observation.to_string(),
                        ..Default::default()
                    };
                    if message.request.operation == Operation::AgentWrite as i32 {
                        assert!(
                            message
                                .gate
                                .as_ref()
                                .is_none_or(|gate| *gate.lock().unwrap())
                        );
                        writer
                            .write_all(format!("{}\r", message.request.text).as_bytes())
                            .unwrap();
                        writer.flush().unwrap();
                        observed.fetch_add(1, Ordering::AcqRel);
                    }
                    let _ = message.reply.send(Reply {
                        info: Some(info),
                        snapshot: Some(Snapshot {
                            epoch: 1,
                            revision: observed.load(Ordering::Acquire),
                            rows: 24,
                            cols: 80,
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                }
                let _ = child.kill();
                let _ = child.wait();
                drop(pair.master);
            });
            host.sessions.lock().unwrap().insert("s".into(), actor);
            let backend = Backend::new(
                &host,
                scope.clone(),
                "device".into(),
                Arc::new(OwnerConfig::default()),
                1,
                Provider {
                    id: "fixture".into(),
                    name: "fixture".into(),
                    connection: Connection {
                        protocol: Protocol::OpenaiChat,
                        endpoint: "http://localhost".into(),
                        api_version: None,
                    },
                    catalog_url: None,
                    secret_ref: None,
                    credential_revision: 1,
                    enabled: true,
                },
                None,
                None,
            )
            .unwrap();
            let (cancel, receiver) = tokio::sync::watch::channel(false);
            let context = ToolContext {
                history_unit_id: root.user_message_id,
                vision: false,
                scope: scope.clone(),
                run_id: root.run_id,
                root_user_message_id: root.root_user_message_id,
                action_id: "command".into(),
                max_read_bytes: 4096,
                budget: Arc::new(Budget::new(30, 10, 10000, scope)),
                cancel: receiver,
                execution_gate: Arc::new(Mutex::new(true)),
                authorization_check: None,
            };
            Self {
                backend,
                context,
                cancel,
                host,
                revision,
                _temp: temp,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.host.sessions.lock().unwrap().clear();
        }
    }
    #[tokio::test]
    async fn submitted_command_returns_actual_correlated_pty_exit_and_persistent_result() {
        let fixture = Fixture::new();
        let accepted = fixture
            .backend
            .invoke_added(&fixture.context, "run_command", json!({"command":"false"}))
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(accepted["accepted"], true);
        assert_eq!(accepted["completion"], "unknown");
        let id = accepted["command_id"].as_str().unwrap();
        let completed = fixture
            .backend
            .invoke_added(
                &fixture.context,
                "wait_command",
                json!({"command_id":id,"timeout_ms":30000}),
            )
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(completed["state"], "completed");
        assert_eq!(completed["exit_code"], 1);
        assert_eq!(completed["timed_out"], false);
        assert_eq!(completed["shell_evidence"]["command"], "false");
        assert!(completed["evidence_event_id"].is_string());
        fixture
            .host
            .agents
            .store
            .finish_run(&fixture.context.scope, &fixture.context.run_id, "completed")
            .unwrap();
        let old = fixture
            .backend
            .invoke_added(
                &fixture.context,
                "get_command_result",
                json!({"command_id":id}),
            )
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(old["exit_code"], 1);
        assert_eq!(old["application_task"]["state"], "unknown");
    }
    #[tokio::test]
    async fn terminal_wait_sees_updates_before_wait_timeout_and_cancel() {
        let fixture = Fixture::new();
        fixture.revision.store(11, Ordering::Release);
        let changed = fixture
            .backend
            .invoke_added(
                &fixture.context,
                "wait_terminal",
                json!({"after_revision":10,"timeout_ms":30000}),
            )
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(changed["changed"], true);
        assert_eq!(changed["timed_out"], false);
        assert!(changed["elapsed_ms"].as_u64().unwrap() < 1000);
        let timeout = fixture
            .backend
            .invoke_added(
                &fixture.context,
                "wait_terminal",
                json!({"after_revision":11,"timeout_ms":10}),
            )
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(timeout["timed_out"], true);
        assert_eq!(timeout["changed"], false);
        let cancel = fixture.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel.send(true).unwrap();
        });
        assert_eq!(
            fixture
                .backend
                .invoke_added(
                    &fixture.context,
                    "wait_terminal",
                    json!({"after_revision":11,"timeout_ms":30000})
                )
                .await
                .err()
                .unwrap()
                .to_string(),
            "cancelled"
        );
    }
    #[tokio::test]
    async fn capabilities_report_bound_role_nonvision_and_budget() {
        let fixture = Fixture::new();
        let value = fixture
            .backend
            .invoke_added(&fixture.context, "get_capabilities", json!({}))
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(value["role"], "session");
        assert_eq!(value["vision"], false);
        assert_eq!(value["terminal"]["application_completion"], false);
        assert_eq!(value["terminal"]["session_id"], "s");
        assert!(
            value["tools"]
                .as_array()
                .unwrap()
                .contains(&json!("run_command"))
        );
        assert!(value["budget"]["remaining_ms"].is_number());
        assert!(
            fixture
                .backend
                .invoke_added(
                    &fixture.context,
                    "get_capabilities",
                    json!({"session_id":"other"})
                )
                .await
                .is_err()
        );
    }
}
