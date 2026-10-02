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
        let result = crate::process::inspect(
            context,
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
            json!({"session_id":session,"desktop_attached":info.desktop_attached,"shell_hooks":shell(&info),"application_task_adapter":null,"application_completion":false})
        } else {
            Value::Null
        };
        Ok(ToolOutput::value(
            json!({"role":if global{"global"}else{"session"},"tools":terminal_tools(global).iter().map(|t|t.name.as_str()).collect::<Vec<_>>(),"vision":context.vision,"terminal":terminal,"authorization":self.host()?.agents.permission_capabilities(context)?,"budget":context.budget.tool_status()?,"limits":{"wait_ms":30000,"task_ids":32,"history_page":50,"shell_evidence":"observational","os_sandbox":false,"cwd_sandbox":false,"mcp_catalog_may_start_enabled_servers":true}}),
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
