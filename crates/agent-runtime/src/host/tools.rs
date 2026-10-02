//! Added builtin schemas and shared-budget task helpers. Parent owns registration/routing.
use super::*;

pub fn definitions(global: bool) -> Vec<ToolDefinition> {
    let timeout = json!({"type":"integer","minimum":1,"maximum":30000});
    let id = json!({"type":"string","minLength":1,"maxLength":128});
    let ids = json!({"type":"array","minItems":1,"maxItems":32,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":36}});
    let mut specs = vec![
        (
            "run_command",
            "Submit a complete command through the existing PTY and authorization gate. Returns command_id/accepted, NOT completion. Only a matching shell sequence and exact command can establish completion; unknown TUI tasks stay unknown.",
            json!({"command":{"type":"string","minLength":1,"maxLength":16000}}),
            vec!["command"],
            true,
        ),
        (
            "get_command_result",
            "Read the exact submitted command result. Unknown/mismatched/missing shell evidence cannot prove completion. Shell completion excludes background jobs and application tasks.",
            json!({"command_id":id}),
            vec!["command_id"],
            false,
        ),
        (
            "wait_command",
            "Wait for this exact command's shell evidence within cancellation and shared budget. Timeout never stops the command; unknown evidence returns unknown.",
            json!({"command_id":id,"timeout_ms":timeout}),
            vec!["command_id", "timeout_ms"],
            false,
        ),
        (
            "ask_user",
            "Ask the real user a bounded question and suspend until their UI/CLI answer, cancellation or expiry. The answer is input, not an operation authorization.",
            json!({"question":{"type":"string","minLength":1,"maxLength":4000},"options":{"type":"array","minItems":2,"maxItems":8,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":500}}}),
            vec!["question"],
            false,
        ),
        (
            "search_history",
            "Search retained history text. Results contain bounded snippets and exact event/record IDs. Cursors bind filters and retention generation; empty pages can have a cursor.",
            json!({"query":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","minLength":1,"maxLength":64},"after_ms":{"type":"integer"},"before_ms":{"type":"integer"},"limit":{"type":"integer","minimum":1,"maximum":50},"cursor":{"type":"string","maxLength":4096}}),
            vec!["query"],
            false,
        ),
        (
            "wait_terminal",
            "Wait for revision > after_revision, including updates that happened before this call. Returns changed/timeout; change is not completion evidence.",
            json!({"after_revision":{"type":"integer","minimum":0},"timeout_ms":timeout}),
            vec!["after_revision", "timeout_ms"],
            true,
        ),
        (
            "get_capabilities",
            "Query actual role, tool catalog, vision, shell hooks, application adapters, authorization and remaining shared budget. Shell reports are observations, not authority.",
            json!({}),
            vec![],
            false,
        ),
    ];
    if global {
        specs.extend([
            ("list_agent_tasks","List delegated Run IDs in this account/Desktop with bounded pagination.",json!({"root_user_message_id":id,"state":{"type":"string","maxLength":64},"limit":{"type":"integer","minimum":1,"maximum":50},"cursor":{"type":"string","maxLength":4096}}),vec![],false),
            ("get_agent_tasks","Read exact delegated Run IDs, preserving caller order. Newer Session runs never replace an older task.",json!({"task_ids":ids}),vec!["task_ids"],false),
            ("wait_agent_tasks","Wait for any or all of a fixed set of exact Run IDs. Timeout does not cancel; inspect each task state/error rather than assuming success.",json!({"task_ids":ids,"mode":{"enum":["any","all"]},"timeout_ms":timeout}),vec!["task_ids","mode","timeout_ms"],false),
            ("cancel_agent_task","Cancel exactly the specified delegated Run, never a newer Run in its Session. Does not send Ctrl+C.",json!({"task_id":id}),vec!["task_id"],false),
        ]);
    }
    let mut tools = specs.into_iter().map(|(name,description,mut properties,mut required,target)| {
        if global && (target || matches!(name,"get_capabilities"|"search_history")) {
            properties["session_id"]=id.clone();
            if target {required.push("session_id");}
        }
        ToolDefinition{name:name.into(),description:description.into(),parameters:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}
    }).collect::<Vec<_>>();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}
/// Apply role contracts to legacy terminal schemas and enable the delta route.
pub fn extend_catalog(tools: &mut Vec<ToolDefinition>, global: bool) {
    for tool in tools.iter_mut() {
        if tool.name == "read_terminal" {
            tool.parameters["properties"]["mode"] =
                json!({"enum":["tail","search","screen","delta"]});
            tool.parameters["properties"]["after_revision"] = json!({"type":"integer","minimum":0});
            tool.description.push_str(" Delta requires after_revision and an observed view_id; changed screens or expired evidence return refetch_required, never fabricated append logs.");
        }
        if matches!(
            tool.name.as_str(),
            "read_terminal" | "get_terminal_state" | "input_text" | "send_keys"
        ) {
            if global {
                let required = tool.parameters["required"].as_array_mut().unwrap();
                if !required.iter().any(|v| v == "session_id") {
                    required.push(json!("session_id"));
                }
            } else {
                tool.parameters["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("session_id");
            }
        }
    }
    tools.extend(definitions(global));
    tools.sort_by(|a, b| a.name.cmp(&b.name));
}
pub fn is_read(name: &str) -> bool {
    matches!(
        name,
        "get_command_result"
            | "wait_command"
            | "list_agent_tasks"
            | "get_agent_tasks"
            | "wait_agent_tasks"
            | "ask_user"
            | "search_history"
            | "wait_terminal"
            | "get_capabilities"
    )
}
impl Budget {
    pub fn tool_status(&self) -> Result<Value> {
        Ok(
            json!({"remaining_ms":self.remaining()?.as_millis() as u64,"model_rounds_remaining":self.max_calls.saturating_sub(self.calls.load(Ordering::Acquire)),"tokens_remaining":self.max_tokens.saturating_sub(self.tokens.load(Ordering::Acquire)),"tool_calls_remaining":256u32.saturating_sub(self.tools.load(Ordering::Acquire)),"read_bytes_remaining":(8*1024*1024u64).saturating_sub(self.reads.load(Ordering::Acquire)),"writes_disabled":self.write_disabled.load(Ordering::Acquire)}),
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    task_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitBatch {
    task_ids: Vec<String>,
    mode: String,
    timeout_ms: u64,
}
fn validate_ids(ids: &[String]) -> Result<()> {
    ensure!((1..=32).contains(&ids.len()), "invalid_task_ids");
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        Uuid::parse_str(id).context("invalid_agent_task_id")?;
        ensure!(seen.insert(id), "duplicate_task_id");
    }
    Ok(())
}
fn check(context: &ToolContext) -> Result<()> {
    context.budget.remaining()?;
    ensure!(!*context.cancel.borrow(), "cancelled");
    Ok(())
}
fn output(context: &ToolContext, value: Value) -> Result<ToolOutput> {
    check(context)?;
    context.budget.read(serde_json::to_vec(&value)?.len())?;
    Ok(ToolOutput::value(value))
}
impl AgentHost {
    pub fn list_agent_tasks(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        check(context)?;
        output(context, self.store.list_agent_tasks(&context.scope, args)?)
    }
    pub fn search_history(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        check(context)?;
        output(context, self.store.search_history(&context.scope, args)?)
    }
    pub fn get_agent_tasks(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let args: Batch = serde_json::from_value(args)?;
        validate_ids(&args.task_ids)?;
        check(context)?;
        let mut bounded = context.clone();
        bounded.max_read_bytes = (context.max_read_bytes / args.task_ids.len()).clamp(4, 2048);
        let tasks = args
            .task_ids
            .iter()
            .map(|id| self.task_state(&bounded, id))
            .collect::<Result<Vec<_>>>()?;
        output(context, json!({"tasks":tasks}))
    }
    pub async fn wait_agent_tasks(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        let args: WaitBatch = serde_json::from_value(args)?;
        validate_ids(&args.task_ids)?;
        ensure!(
            matches!(args.mode.as_str(), "any" | "all"),
            "invalid_task_wait_mode"
        );
        ensure!(
            (1..=30000).contains(&args.timeout_ms),
            "invalid_task_wait_timeout"
        );
        let started = Instant::now();
        let end = started + Duration::from_millis(args.timeout_ms);
        let mut updates = self.task_updates.subscribe();
        let mut cancel = context.cancel.clone();
        let mut bounded = context.clone();
        bounded.max_read_bytes = (context.max_read_bytes / args.task_ids.len()).clamp(4, 2048);
        loop {
            check(context)?;
            let tasks = args
                .task_ids
                .iter()
                .map(|id| self.task_state(&bounded, id))
                .collect::<Result<Vec<_>>>()?;
            let done = if args.mode == "all" {
                tasks.iter().all(|v| v["done"] == true)
            } else {
                tasks.iter().any(|v| v["done"] == true)
            };
            if done || Instant::now() >= end {
                return output(
                    context,
                    json!({"tasks":tasks,"mode":args.mode,"timed_out":!done,"elapsed_ms":started.elapsed().as_millis() as u64}),
                );
            }
            let remaining = context.budget.remaining()?;
            tokio::select! {biased; _=cancel.wait_for(|v|*v)=>bail!("cancelled"), _=tokio::time::sleep(remaining)=>bail!("run_time_budget"), _=tokio::time::sleep_until(tokio::time::Instant::from_std(end))=>{}, changed=updates.changed()=>{changed.context("agent_task_notifications_closed")?;}}
        }
    }
    pub fn cancel_agent_task(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Args {
            task_id: String,
        }
        let args: Args = serde_json::from_value(args)?;
        check(context)?;
        let (target, _) =
            self.store
                .agent_task_for_run(&context.scope, &context.run_id, &args.task_id, 4)?;
        let jobs = self.jobs.lock().unwrap();
        let mut cancelled = false;
        if let Some(job) = jobs.get(&target.agent)
            && job.run == args.task_id
        {
            ensure!(job.scope == target, "agent_scope_mismatch");
            let mut state = job.state.lock().unwrap();
            if running(&state.state) {
                *job.execution_gate.lock().unwrap() = false;
                let _ = job.cancel.send(true);
                state.state = "stopping".into();
                cancelled = true;
            }
        }
        // Never call cancel(scope): it could stop a newer Run after this lookup.
        drop(jobs);
        self.task_updates.send_replace(());
        output(
            context,
            json!({"task_id":args.task_id,"cancel_requested":cancelled,"terminal_interrupt_sent":false}),
        )
    }
}
