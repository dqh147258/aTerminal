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
        context
            .budget
            .waiting_for_tasks(&context.run_id, self.wait_agent_tasks_inner(context, args))
            .await
    }
    async fn wait_agent_tasks_inner(
        &self,
        context: &ToolContext,
        args: Value,
    ) -> Result<ToolOutput> {
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
        let timeout = Duration::from_millis(args.timeout_ms);
        let initial_remaining = context.budget.remaining()?;
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
            let remaining = context.budget.remaining()?;
            let active_elapsed = initial_remaining.saturating_sub(remaining);
            if done || active_elapsed >= timeout {
                return output(
                    context,
                    json!({"tasks":tasks,"mode":args.mode,"timed_out":!done,"elapsed_ms":started.elapsed().as_millis() as u64,"active_elapsed_ms":active_elapsed.as_millis() as u64}),
                );
            }
            tokio::select! {biased; _=cancel.wait_for(|v|*v)=>bail!("cancelled"), _=tokio::time::sleep(remaining.min(Duration::from_millis(100)).min(timeout.saturating_sub(active_elapsed)))=>{}, changed=updates.changed()=>{changed.context("agent_task_notifications_closed")?;}}
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
        context.check_authorization()?;
        let jobs = self.jobs.lock().unwrap();
        let mut cancelled = false;
        if let Some(job) = jobs.get(&target.agent)
            && job.run == args.task_id
        {
            ensure!(job.scope == target, "agent_scope_mismatch");
            let mut gate = job.execution_gate.lock().unwrap();
            let mut state = job.state.lock().unwrap();
            if running(&state.state) {
                context.check_authorization()?;
                context.commit_authorization(None)?;
                *gate = false;
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

#[cfg(test)]
mod toolset_tests {
    use super::*;
    struct Fixture {
        host: Arc<AgentHost>,
        context: ToolContext,
        cancel: watch::Sender<bool>,
        child: Scope,
        _temp: tempfile::TempDir,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let store = Arc::new(Store::open(&temp.path().join("data/db")).unwrap());
            let scope = store.agent("o", "d", None).unwrap();
            let child = store.agent("o", "d", Some("s")).unwrap();
            let root = store
                .accept_user(&scope, "r", "coordinate", json!({}))
                .unwrap();
            let (cancel, receiver) = watch::channel(false);
            let context = ToolContext {
                history_unit_id: root.user_message_id.clone(),
                vision: false,
                scope: scope.clone(),
                run_id: root.run_id,
                root_user_message_id: root.root_user_message_id,
                action_id: "a".into(),
                max_read_bytes: 4096,
                budget: Arc::new(Budget::new(30, 20, 20000, scope)),
                cancel: receiver,
                execution_gate: Arc::new(Mutex::new(true)),
                authorization_check: None,
            };
            Self {
                host: AgentHost::new(store, tokio::runtime::Handle::current()),
                context,
                cancel,
                child,
                _temp: temp,
            }
        }
        fn task(&self, target: &Scope) -> String {
            self.host
                .store
                .delegate(
                    target,
                    &self.context.root_user_message_id,
                    &id(),
                    "work",
                    json!({}),
                    None,
                )
                .unwrap()
                .run_id
        }
    }
    #[test]
    fn schemas_enforce_role_targets_and_bounded_batch_parameters() {
        let session = definitions(false);
        let global = definitions(true);
        for name in [
            "list_agent_tasks",
            "get_agent_tasks",
            "wait_agent_tasks",
            "cancel_agent_task",
        ] {
            assert!(!session.iter().any(|t| t.name == name));
            assert!(global.iter().any(|t| t.name == name));
        }
        for name in ["run_command", "wait_terminal"] {
            let global = global.iter().find(|t| t.name == name).unwrap();
            assert!(
                global.parameters["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("session_id"))
            );
            assert!(
                session.iter().find(|t| t.name == name).unwrap().parameters["properties"]
                    .get("session_id")
                    .is_none()
            );
        }
        let tool = global
            .iter()
            .find(|t| t.name == "wait_agent_tasks")
            .unwrap();
        let schema = jsonschema::validator_for(&tool.parameters).unwrap();
        assert!(schema.is_valid(&json!({"task_ids":[id()],"mode":"any","timeout_ms":1})));
        for value in [
            json!({"task_ids":[],"mode":"all","timeout_ms":1}),
            json!({"task_ids":[id()],"mode":"any","timeout_ms":30001}),
            json!({"task_ids":[id()],"mode":"other","timeout_ms":1}),
        ] {
            assert!(!schema.is_valid(&value));
        }
    }
    #[tokio::test]
    async fn batch_any_all_preserve_fixed_ids_timeout_cancel_and_notification() {
        let fixture = Fixture::new();
        let one = fixture.task(&fixture.child);
        let other = fixture.host.store.agent("o", "d", Some("other")).unwrap();
        let two = fixture.task(&other);
        fixture
            .host
            .store
            .finish_run(&fixture.child, &one, "completed")
            .unwrap();
        let any = fixture
            .host
            .wait_agent_tasks(
                &fixture.context,
                json!({"task_ids":[one,two],"mode":"any","timeout_ms":30000}),
            )
            .await
            .unwrap()
            .value;
        assert_eq!(any["timed_out"], false);
        assert_eq!(any["tasks"][0]["task_id"], one);
        assert_eq!(any["tasks"][1]["task_id"], two);
        let all = fixture
            .host
            .wait_agent_tasks(
                &fixture.context,
                json!({"task_ids":[one,two],"mode":"all","timeout_ms":10}),
            )
            .await
            .unwrap()
            .value;
        assert_eq!(all["timed_out"], true);
        assert_eq!(all["tasks"][1]["state"], "running");
        let host = fixture.host.clone();
        let completed = two.clone();
        let target = other.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            host.store
                .finish_run(&target, &completed, "cancelled")
                .unwrap();
            host.task_updates.send_replace(());
        });
        let all = fixture
            .host
            .wait_agent_tasks(
                &fixture.context,
                json!({"task_ids":[one,two],"mode":"all","timeout_ms":30000}),
            )
            .await
            .unwrap()
            .value;
        assert_eq!(all["timed_out"], false);
        assert_eq!(all["tasks"][1]["state"], "cancelled");
        let pending = fixture.task(&other);
        let cancel = fixture.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            cancel.send(true).unwrap();
        });
        assert_eq!(
            fixture
                .host
                .wait_agent_tasks(
                    &fixture.context,
                    json!({"task_ids":[pending],"mode":"all","timeout_ms":30000})
                )
                .await
                .err()
                .unwrap()
                .to_string(),
            "cancelled"
        );
    }
    #[tokio::test]
    async fn batch_invalid_or_foreign_ids_fail_as_a_set() {
        let fixture = Fixture::new();
        let task = fixture.task(&fixture.child);
        let foreign = fixture.host.store.agent("foreign", "d", Some("s")).unwrap();
        let bad = fixture.task(&foreign);
        for ids in [
            vec![task.clone(), task.clone()],
            vec![task.clone(), bad],
            vec![task.clone(), id()],
        ] {
            assert!(
                fixture
                    .host
                    .get_agent_tasks(&fixture.context, json!({"task_ids":ids}))
                    .is_err()
            );
        }
        assert!(
            fixture
                .host
                .list_agent_tasks(&fixture.context, json!({"limit":51}))
                .is_err()
        );
        let listed = fixture
            .host
            .list_agent_tasks(&fixture.context, json!({}))
            .unwrap()
            .value;
        assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed["tasks"][0]["task_id"], task);
    }
    struct UnusedModel;
    impl Model for UnusedModel {
        fn stream(
            &self,
            _: rig_core::completion::CompletionRequest,
        ) -> BackendFuture<'_, rig_core::streaming::StreamingCompletionResponse> {
            Box::pin(async { bail!("unused_fixture_model") })
        }
    }
    struct UnusedBackend;
    impl TerminalBackend for UnusedBackend {
        fn authorize(&self, _: bool) -> BackendFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
        fn invoke<'a>(
            &'a self,
            _: ToolContext,
            _: &'a str,
            _: Value,
        ) -> BackendFuture<'a, ToolOutput> {
            Box::pin(async { bail!("unused_fixture_backend") })
        }
    }
    #[tokio::test]
    async fn cancelling_old_task_does_not_touch_newer_session_run() {
        let fixture = Fixture::new();
        let old = fixture.task(&fixture.child);
        fixture
            .host
            .store
            .finish_run(&fixture.child, &old, "completed")
            .unwrap();
        let new = fixture.task(&fixture.child);
        let (cancel, _receiver) = watch::channel(false);
        let snapshot = RunSnapshot {
            revision: 1,
            provider: Protocol::OpenaiChat,
            builder: RequestBuilder {
                settings: crate::model::RequestSettings {
                    model: "unused".into(),
                    temperature: None,
                    max_tokens: 100,
                    additional_params: None,
                },
                system: "fixture".into(),
                tools: vec![],
            },
            model: Arc::new(UnusedModel),
            backend: Arc::new(UnusedBackend),
            context_window: 10000,
            max_rounds: 1,
            max_seconds: 30,
            allow_write: true,
            vision: false,
        };
        let job = Arc::new(Job {
            scope: fixture.child.clone(),
            run: new.clone(),
            root: fixture.context.root_user_message_id.clone(),
            snapshot: Arc::new(snapshot),
            budget: fixture.context.budget.clone(),
            cancel: cancel.clone(),
            state: Mutex::new(JobState {
                state: "running".into(),
                queue: VecDeque::new(),
                live: String::new(),
                error: None,
            }),
            execution_gate: Arc::new(Mutex::new(true)),
        });
        fixture
            .host
            .jobs
            .lock()
            .unwrap()
            .insert(fixture.child.agent.clone(), job.clone());
        assert_eq!(
            fixture
                .host
                .cancel_agent_task(&fixture.context, json!({"task_id":old}))
                .unwrap()
                .value["cancel_requested"],
            false
        );
        assert!(!*cancel.borrow());
        assert!(*job.execution_gate.lock().unwrap());
        assert_eq!(
            fixture
                .host
                .cancel_agent_task(&fixture.context, json!({"task_id":new}))
                .unwrap()
                .value["cancel_requested"],
            true
        );
        assert!(*cancel.borrow());
        assert!(!*job.execution_gate.lock().unwrap());
        assert!(!fixture.context.budget.cancelled.load(Ordering::Acquire));
    }
}
