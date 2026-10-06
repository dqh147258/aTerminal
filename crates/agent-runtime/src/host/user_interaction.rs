use super::*;
use crate::authorization::{Risk, assess};
use std::collections::HashSet;

type ExecutionCheck = Arc<dyn Fn() -> Result<()> + Send + Sync>;
type CommitCheck = Arc<dyn Fn(Option<&Value>) -> Result<()> + Send + Sync>;
/// A host-issued capability for one actual side effect. It is never deserializable.
pub struct AuthorizationPermit {
    check: ExecutionCheck,
    commit: CommitCheck,
    consumed: AtomicBool,
    command: Option<String>,
}
impl AuthorizationPermit {
    /// A trusted native observation has no approval mutation, but still revalidates
    /// identity/cwd at the actual spawn boundary and can only launch once.
    pub fn observation(check: impl Fn() -> Result<()> + Send + Sync + 'static) -> Arc<Self> {
        let check: ExecutionCheck = Arc::new(check);
        let commit_check = check.clone();
        Arc::new(Self {
            check,
            commit: Arc::new(move |_| commit_check()),
            consumed: AtomicBool::new(false),
            command: None,
        })
    }
    pub fn submitted_command(&self) -> Option<&str> {
        self.command.as_deref()
    }
    pub fn check(&self) -> Result<()> {
        (self.check)()
    }
    pub fn commit(&self, observation: Option<&Value>) -> Result<()> {
        (self.commit)(observation)?;
        ensure!(
            !self.consumed.swap(true, Ordering::AcqRel),
            "authorization_action_already_consumed"
        );
        Ok(())
    }
}

pub(super) struct ActiveClock {
    last: Instant,
    pub(super) paused: Duration,
    human: HashSet<String>,
    dependencies: HashSet<String>,
}
impl Default for ActiveClock {
    fn default() -> Self {
        Self {
            last: Instant::now(),
            paused: Duration::ZERO,
            human: HashSet::new(),
            dependencies: HashSet::new(),
        }
    }
}
impl ActiveClock {
    pub(super) fn tick(&mut self, active: u32) {
        let now = Instant::now();
        if !self.human.is_empty() && self.human.union(&self.dependencies).count() >= active as usize
        {
            self.paused += now.saturating_duration_since(self.last);
        }
        self.last = now;
    }
}
struct Waiting<'a> {
    budget: &'a Budget,
    run: String,
    human: bool,
}
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let mut clock = self.budget.clock.lock().unwrap();
        clock.tick(self.budget.active.load(Ordering::Acquire));
        if self.human {
            clock.human.remove(&self.run);
        } else {
            clock.dependencies.remove(&self.run);
        }
    }
}
impl Budget {
    pub(super) fn change_active(&self, add: bool) -> u32 {
        let mut clock = self.clock.lock().unwrap();
        clock.tick(self.active.load(Ordering::Acquire));
        if add {
            self.active.fetch_add(1, Ordering::AcqRel)
        } else {
            self.active.fetch_sub(1, Ordering::AcqRel)
        }
    }
    fn waiting(&self, run: &str, human: bool) -> Waiting<'_> {
        let mut clock = self.clock.lock().unwrap();
        clock.tick(self.active.load(Ordering::Acquire));
        if human {
            clock.human.insert(run.into());
        } else {
            clock.dependencies.insert(run.into());
        }
        Waiting {
            budget: self,
            run: run.into(),
            human,
        }
    }
    /// Recomputes the shared active deadline while a human interaction suspends the tree.
    pub async fn run_bounded<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::pin!(future);
        loop {
            let remaining = self.remaining()?;
            tokio::select! { biased; result=&mut future=>return result, _=tokio::time::sleep(remaining.min(Duration::from_millis(100)))=>{} }
        }
    }
    pub async fn waiting_for_tasks<T>(
        &self,
        run: &str,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let _waiting = self.waiting(run, false);
        self.run_bounded(future).await
    }
    pub fn permission_scope(&self) -> &Scope {
        &self.root_scope
    }
}
impl ToolContext {
    /// Linearize the last live authorization check with the actual synchronous
    /// effect. Callers needing `jobs` must acquire it first, like permission RPCs.
    pub fn commit_effect<T>(&self, effect: impl FnOnce() -> Result<T>) -> Result<T> {
        let gate = self.execution_gate.lock().unwrap();
        ensure!(*gate && !*self.cancel.borrow(), "execution_revoked");
        self.commit_authorization(None)?;
        effect()
    }
    pub fn commit_authorization(&self, observation: Option<&Value>) -> Result<()> {
        self.budget.remaining()?;
        ensure!(!*self.cancel.borrow(), "cancelled");
        if let Some(permit) = &self.authorization_check {
            permit.commit(observation)?;
        }
        Ok(())
    }
    pub fn check_authorization(&self) -> Result<()> {
        self.budget.remaining()?;
        ensure!(!*self.cancel.borrow(), "cancelled");
        ensure!(*self.execution_gate.lock().unwrap(), "execution_revoked");
        if let Some(check) = &self.authorization_check {
            check.check()?;
        }
        Ok(())
    }
}
impl AgentHost {
    pub fn set_permissions(
        &self,
        scope: &Scope,
        expected: u64,
        mode: Option<&str>,
        full: Option<bool>,
    ) -> Result<Value> {
        let jobs = self.jobs.lock().unwrap();
        let gates = jobs
            .values()
            .filter(|j| j.budget.permission_scope() == scope)
            .map(|j| j.execution_gate.clone())
            .collect::<Vec<_>>();
        let permits = gates.iter().map(|g| g.lock().unwrap()).collect::<Vec<_>>();
        let result = self.store.set_permissions(scope, expected, mode, full);
        drop(permits);
        drop(jobs);
        self.permissions_changed(scope);
        result
    }
    pub fn revoke_rule(&self, scope: &Scope, request: &str, rule: &str) -> Result<Value> {
        let jobs = self.jobs.lock().unwrap();
        let gates = jobs
            .values()
            .filter(|j| j.scope.owner == scope.owner && j.scope.desktop == scope.desktop)
            .map(|j| j.execution_gate.clone())
            .collect::<Vec<_>>();
        let permits = gates.iter().map(|g| g.lock().unwrap()).collect::<Vec<_>>();
        let result = self.store.revoke_rule_request(scope, request, rule);
        drop(permits);
        drop(jobs);
        result
    }
    pub fn human_response_changed(&self) {
        self.human_updates.send_replace(());
    }
    pub fn permissions_changed(&self, _scope: &Scope) {
        self.human_response_changed();
    }
    pub fn permission_capabilities(&self, context: &ToolContext) -> Result<Value> {
        let mut value = self.store.permissions(context.budget.permission_scope())?;
        value["remaining_ms"] = json!(context.budget.remaining()?.as_millis() as u64);
        value["approval_ttl_ms"] = json!(24 * 60 * 60 * 1000u64);
        Ok(value)
    }
    async fn await_human(&self, context: &ToolContext, pending: Value) -> Result<Value> {
        let id = pending["id"].as_str().context("pending_id_required")?;
        let mut updates = self.human_updates.subscribe();
        let job = self
            .jobs
            .lock()
            .unwrap()
            .get(&context.scope.agent)
            .cloned()
            .context("run_not_found")?;
        ensure!(job.run == context.run_id, "run_changed");
        job.state.lock().unwrap().state = "waiting_for_user".into();
        let result = async {
            let _waiting = context.budget.waiting(&context.run_id, true);
            let mut cancel = context.cancel.clone();
            loop {
                context.budget.remaining()?;
                ensure!(!*cancel.borrow(), "cancelled");
                tokio::select! {biased; _=cancel.wait_for(|v|*v)=>bail!("cancelled"), result=job.snapshot.backend.authorize(false)=>{result?;}}
                context.budget.remaining()?;
                ensure!(!*cancel.borrow()&&*context.execution_gate.lock().unwrap(),"cancelled");
                let item = self.store.poll_pending(&context.scope, id)?;
                if item["state"] == "pending" && item["kind"] == "approval" {
                    let mode = self.store.permissions(context.budget.permission_scope())?;
                    ensure!(
                        mode["permission_mode"] != "read_only",
                        "terminal_write_not_authorized"
                    );
                    if mode["full_authorization"] == true {
                        self.store.supersede_approval(&context.scope, id)?;
                        return Ok(json!({"decision":"full"}));
                    }
                }
                match item["state"].as_str() {
                    Some("resolved") => return self.store.consume_pending(&context.scope, id),
                    Some("pending") => {}
                    _ => bail!(
                        "human_request_{}",
                        item["state"].as_str().unwrap_or("unavailable")
                    ),
                }
                tokio::select! { biased;
                    _=cancel.wait_for(|v|*v)=>bail!("cancelled"),
                    _=updates.changed()=>{},
                    _=tokio::time::sleep(Duration::from_millis(100))=>{},
                }
            }
        }
        .await;
        let mut state = job.state.lock().unwrap();
        if state.state == "waiting_for_user" {
            state.state = "running".into();
        }
        result
    }
    pub async fn ask_user(&self, context: &ToolContext, args: Value) -> Result<ToolOutput> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Question {
            question: String,
            options: Option<Vec<String>>,
        }
        let question: Question = serde_json::from_value(args)?;
        ensure!(
            !question.question.trim().is_empty() && question.question.len() <= 4096,
            "invalid_question"
        );
        ensure!(
            question
                .options
                .as_ref()
                .is_none_or(|options| options.len() <= 16
                    && options
                        .iter()
                        .all(|s| !s.trim().is_empty() && s.len() <= 512)),
            "invalid_question_options"
        );
        let pending=self.store.create_pending(&context.scope,context.budget.permission_scope(),&context.run_id,&context.action_id,json!({"kind":"question","title":"Agent question","reason":"user_input_required","question":question.question,"options":question.options}))?;
        let response = self.await_human(context, pending).await?;
        Ok(ToolOutput::value(
            json!({"answer":response["answer"],"source":"authenticated_user"}),
        ))
    }
    pub(super) async fn approve_action(
        &self,
        job: &Arc<Job>,
        context: &ToolContext,
        name: &str,
        args: &Value,
    ) -> Result<Arc<AuthorizationPermit>> {
        let authority = context.budget.permission_scope();
        let mode = self.store.permissions(authority)?;
        ensure!(
            mode["permission_mode"] != "read_only",
            "terminal_write_not_authorized"
        );
        let descriptor = job
            .snapshot
            .backend
            .action_descriptor(context, name, args)?;
        let expected_fence = job.snapshot.backend.action_fence(context, name, args)?;
        let assessment = assess(&descriptor);
        ensure!(
            assessment.risk != Risk::Forbidden,
            "authorization_forbidden: {}",
            assessment.reason
        );
        ensure!(
            mode["full_authorization"] == true
                || !self.store.action_denied(
                    &context.scope,
                    &context.run_id,
                    &assessment.fingerprint
                )?,
            "authorization_denied"
        );
        let mut once = false;
        if assessment.risk != Risk::Safe
            && mode["full_authorization"] != true
            && !(assessment.can_always
                && self
                    .store
                    .rule_matches(&context.scope, &assessment.fingerprint)?)
        {
            let (preview, details) = job.snapshot.backend.approval_display(args)?;
            let requires_details = details.to_string().len() > 4096;
            let pending=self.store.create_pending(&context.scope,authority,&context.run_id,&context.action_id,json!({"kind":"approval","title":format!("Approve {name}"),"reason":assessment.reason,"tool":name,"arguments_preview":preview,"arguments_truncated":requires_details,"requires_details":requires_details,"_details":details,"cwd":descriptor.cwd,"fingerprint":assessment.fingerprint,"can_always":assessment.can_always,"always_unavailable_reason":if assessment.can_always {Value::Null} else if descriptor.source==crate::authorization::ToolSource::Skill {json!("脚本有效依赖版本无法完全固定，仅可一次/完全授权")} else {json!("Exact execution identity or cwd unavailable")},"rule_preview":{"tool":name,"target":descriptor.target,"cwd":descriptor.cwd,"source":descriptor.source_id,"version":descriptor.tool_version,"execution_identity":descriptor.execution_identity,"dispatch":format!("{:?}",descriptor.source)}}))?;
            let response = self.await_human(context, pending).await?;
            ensure!(response["decision"] != "deny", "authorization_denied");
            once = response["decision"] == "once";
        }
        let expected = assessment.fingerprint;
        let store = self.store.clone();
        let scope = context.scope.clone();
        let authority = authority.clone();
        let backend = job.snapshot.backend.clone();
        let context = context.clone();
        let name = name.to_owned();
        let args = args.clone();
        let check_context = context.clone();
        let initially_safe = assessment.risk == Risk::Safe;
        let rule_eligible = assessment.can_always;
        let common: ExecutionCheck = Arc::new(move || {
            ensure!(!*context.cancel.borrow(), "cancelled");
            context.budget.remaining()?;
            ensure!(
                !context.budget.write_disabled.load(Ordering::Acquire),
                "write_authorization_revoked"
            );
            let mode = store.permissions(&authority)?;
            ensure!(
                mode["permission_mode"] != "read_only",
                "terminal_write_not_authorized"
            );
            ensure!(
                once || initially_safe
                    || mode["full_authorization"] == true
                    || (rule_eligible && store.rule_matches(&scope, &expected)?),
                "authorization_revoked"
            );
            Ok(())
        });
        let expected = assessment_fingerprint(&descriptor);
        let check_common = common.clone();
        let check_descriptor = descriptor.clone();
        let check_fence = expected_fence.clone();
        let check_store = self.store.clone();
        let check_authority = job.budget.permission_scope().clone();
        let check: ExecutionCheck = Arc::new(move || {
            check_common()?;
            let current = backend.action_descriptor(&check_context, &name, &args)?;
            ensure!(
                assess(&current).risk != Risk::Forbidden,
                "authorization_forbidden"
            );
            ensure!(
                assessment_fingerprint(&current) == expected,
                "authorization_context_changed"
            );
            if initially_safe {
                ensure!(
                    assess(&current).risk == Risk::Safe,
                    "authorization_context_changed"
                );
            }
            ensure!(
                backend.action_fence(&check_context, &name, &args)? == check_fence,
                "authorization_input_changed"
            );
            if check_descriptor.tool == "run_command"
                && !once
                && !initially_safe
                && check_store.permissions(&check_authority)?["full_authorization"] != true
            {
                ensure!(
                    assess(&current).can_always,
                    "authorization_rule_scope_changed"
                );
            }
            Ok(())
        });
        let submitted_command = if descriptor.tool == "run_command" {
            descriptor.arguments["command"].as_str().map(str::to_owned)
        } else {
            None
        };
        let commit_check = check.clone();
        let commit_store = self.store.clone();
        let commit_authority = job.budget.permission_scope().clone();
        let commit: CommitCheck = Arc::new(move |observation| {
            common()?;
            if observation.is_none() {
                commit_check()?;
            }
            if let Some(observation) = observation {
                ensure!(
                    observation["cwd"] == json!(descriptor.cwd),
                    "authorization_cwd_changed"
                );
                if descriptor.tool == "run_command"
                    && let Some(expected) = &descriptor.execution_identity
                {
                    let program = descriptor.arguments["command"]
                        .as_str()
                        .and_then(crate::authorization::permanent_command_program)
                        .context("authorization_program_changed")?;
                    ensure!(
                        blake3::hash(&std::fs::read(program)?).to_hex().as_str() == expected,
                        "authorization_program_changed"
                    );
                }
                if !expected_fence.is_null() {
                    ensure!(
                        observation["fence"] == expected_fence,
                        "authorization_input_changed"
                    );
                }
                if descriptor.tool == "run_command"
                    && !once
                    && commit_store.permissions(&commit_authority)?["full_authorization"] != true
                {
                    ensure!(
                        observation["input_buffer_empty"] == true
                            && observation["phase"] == "prompt",
                        "authorization_input_changed"
                    );
                }
            }
            Ok(())
        });
        job.snapshot.backend.authorize(true).await?;
        check()?;
        Ok(Arc::new(AuthorizationPermit {
            check,
            commit,
            consumed: AtomicBool::new(false),
            command: submitted_command,
        }))
    }
}
fn assessment_fingerprint(descriptor: &crate::authorization::ActionDescriptor) -> String {
    crate::authorization::stable_fingerprint(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::streaming::{
        RawStreamingChoice as Raw, RawStreamingToolCall, StreamFinal, StreamingCompletionResponse,
    };
    struct ModelStub {
        calls: AtomicU32,
        actions: Vec<(String, Value)>,
    }
    impl Model for ModelStub {
        fn stream(
            &self,
            _request: rig_core::completion::CompletionRequest,
        ) -> BackendFuture<'_, StreamingCompletionResponse> {
            Box::pin(async move {
                let index = self.calls.fetch_add(1, Ordering::AcqRel) as usize;
                let choice = if let Some((name, args)) = self.actions.get(index) {
                    Raw::ToolCall(RawStreamingToolCall::new(
                        format!("call{index}"),
                        name.clone(),
                        args.clone(),
                    ))
                } else {
                    Raw::Message("finished".into())
                };
                Ok(StreamingCompletionResponse::stream(
                    "stub",
                    Box::pin(futures_util::stream::iter(vec![
                        Ok(choice),
                        Ok(Raw::FinalResponse(StreamFinal::new(
                            "stub",
                            Default::default(),
                        ))),
                    ])),
                ))
            })
        }
    }
    #[derive(Default)]
    struct BackendStub {
        writes: AtomicU32,
        cwd: Mutex<String>,
        revoked: AtomicBool,
    }
    impl TerminalBackend for BackendStub {
        fn authorize(&self, _write: bool) -> BackendFuture<'_, ()> {
            Box::pin(async {
                ensure!(!self.revoked.load(Ordering::Acquire), "identity_revoked");
                Ok(())
            })
        }
        fn action_descriptor(
            &self,
            context: &ToolContext,
            name: &str,
            args: &Value,
        ) -> Result<crate::authorization::ActionDescriptor> {
            Ok(crate::authorization::ActionDescriptor {
                account_id: context.scope.owner.clone(),
                desktop_id: context.scope.desktop.clone(),
                tool: name.into(),
                source: crate::authorization::ToolSource::Builtin,
                source_id: if name == "run_program" {
                    "aterminal/native-program.v3".into()
                } else {
                    "builtin-test".into()
                },
                tool_version: Some("v1".into()),
                target: "terminal".into(),
                cwd: Some(self.cwd.lock().unwrap().clone()),
                arguments: args.clone(),
                execution_identity: Some("fixed-test-program".into()),
                shell_proof: None,
                permission_management: false,
            })
        }
        fn invoke<'a>(
            &'a self,
            context: ToolContext,
            _name: &'a str,
            _args: Value,
        ) -> BackendFuture<'a, ToolOutput> {
            Box::pin(async move {
                context.check_authorization()?;
                context.commit_authorization(None)?;
                self.writes.fetch_add(1, Ordering::AcqRel);
                Ok(ToolOutput::value(json!({"accepted":true})))
            })
        }
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        host: Arc<AgentHost>,
        scope: Scope,
        model: Arc<ModelStub>,
        backend: Arc<BackendStub>,
    }
    impl Fixture {
        fn new(actions: Vec<(&str, Value)>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(Store::open(&dir.path().join("private/agent.db")).unwrap());
            let scope = store.agent("owner", "desktop", None).unwrap();
            let host = AgentHost::new(store, tokio::runtime::Handle::current());
            Self {
                _dir: dir,
                host,
                scope,
                model: Arc::new(ModelStub {
                    calls: AtomicU32::new(0),
                    actions: actions.into_iter().map(|(s, a)| (s.into(), a)).collect(),
                }),
                backend: Arc::new(BackendStub {
                    cwd: Mutex::new("/tmp/work".into()),
                    ..Default::default()
                }),
            }
        }
        fn start(&self, request: &str, seconds: u64, write: bool) {
            let model = self.model.clone();
            let backend = self.backend.clone();
            self.host
                .submit(
                    self.scope.clone(),
                    request,
                    "perform work",
                    json!({}),
                    write,
                    "",
                    move || {
                        Ok(RunSnapshot {
                            revision: 1,
                            provider: Protocol::OpenaiChat,
                            builder: RequestBuilder {
                                settings: crate::model::RequestSettings {
                                    model: "stub".into(),
                                    temperature: None,
                                    max_tokens: 2048,
                                    additional_params: None,
                                },
                                system: "fixed".into(),
                                tools: terminal_tools(true),
                            },
                            model,
                            backend,
                            context_window: 128000,
                            max_rounds: 10,
                            max_seconds: seconds,
                            allow_write: write,
                            vision: false,
                        })
                    },
                )
                .unwrap();
        }
        async fn pending(&self) -> Value {
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let page = self.host.store.pending(&self.scope, None).unwrap();
                    if let Some(item) = page["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|v| v["state"] == "pending")
                    {
                        return item.clone();
                    }
                    let state = self.host.state(&self.scope).unwrap();
                    assert!(
                        running(state["state"].as_str().unwrap()),
                        "run ended before creating a pending request: {state}"
                    );
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "pending not created: {}",
                    self.host.state(&self.scope).unwrap()
                )
            })
        }
        async fn settle(&self) -> Value {
            let mut updates = self.host.task_updates.subscribe();
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let state = self.host.state(&self.scope).unwrap();
                    if !running(state["state"].as_str().unwrap()) {
                        return state;
                    }
                    updates.changed().await.unwrap();
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!("run did not settle: {}", self.host.state(&self.scope).unwrap())
            })
        }
        fn answer(&self, item: &Value, decision: &str) {
            self.host
                .store
                .resolve_pending(
                    &self.scope,
                    "response",
                    item["id"].as_str().unwrap(),
                    Some(decision),
                    None,
                )
                .unwrap();
            self.host.human_response_changed();
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn effect_commit_is_ordered_with_acknowledged_permission_changes() {
        let f = Fixture::new(vec![(
            "ask_user",
            json!({"question":"Keep the root alive"}),
        )]);
        f.host
            .set_permissions(&f.scope, 0, None, Some(true))
            .unwrap();
        f.start("gate-root", 30, true);
        f.pending().await;
        let job = f
            .host
            .jobs
            .lock()
            .unwrap()
            .get(&f.scope.agent)
            .unwrap()
            .clone();
        let mut context = ToolContext {
            scope: f.scope.clone(),
            run_id: job.run.clone(),
            root_user_message_id: job.root.clone(),
            history_unit_id: "gate-test".into(),
            action_id: "gate-effect".into(),
            max_read_bytes: 4096,
            vision: false,
            budget: job.budget.clone(),
            cancel: job.cancel.subscribe(),
            execution_gate: job.execution_gate.clone(),
            authorization_check: None,
        };
        context.authorization_check = Some(
            f.host
                .approve_action(
                    &job,
                    &context,
                    "run_program",
                    &json!({"program":"/usr/bin/tee","args":[]}),
                )
                .await
                .unwrap(),
        );
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = context.clone();
        let effect = std::thread::spawn(move || {
            worker.commit_effect(|| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok("effect started")
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let host = f.host.clone();
        let scope = f.scope.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (changed_tx, changed_rx) = std::sync::mpsc::channel();
        let changer = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = host.set_permissions(&scope, 1, Some("read_only"), None);
            changed_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(changed_rx.recv_timeout(Duration::from_millis(50)).is_err());
        release_tx.send(()).unwrap();
        assert_eq!(effect.join().unwrap().unwrap(), "effect started");
        assert_eq!(
            changed_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap()["permission_mode"],
            "read_only"
        );
        changer.join().unwrap();

        // The opposite order must reject the effect after the permission RPC ACK.
        f.host
            .set_permissions(&f.scope, 2, Some("ask"), Some(true))
            .unwrap();
        context.action_id = "gate-revoked".into();
        context.authorization_check = Some(
            f.host
                .approve_action(
                    &job,
                    &context,
                    "run_program",
                    &json!({"program":"/usr/bin/tee","args":[]}),
                )
                .await
                .unwrap(),
        );
        f.host
            .set_permissions(&f.scope, 3, Some("read_only"), None)
            .unwrap();
        let executed = AtomicBool::new(false);
        assert!(
            context
                .commit_effect(|| {
                    executed.store(true, Ordering::Release);
                    Ok(())
                })
                .is_err()
        );
        assert!(!executed.load(Ordering::Acquire));
        f.host.cancel(&f.scope).unwrap();
        f.settle().await;
    }
    fn raw() -> Value {
        json!({"session_id":"terminal","text":"touch file","submit":true})
    }
    #[tokio::test]
    async fn approval_wait_suspends_deadline_without_model_iteration_and_once_executes_once() {
        let fixture = Fixture::new(vec![("input_text", raw())]);
        // Allow real SQLite/MCP work to finish on loaded CI runners before
        // checking suspension across the actual run deadline.
        fixture.start("root", 30, true);
        let pending = fixture.pending().await;
        assert_eq!(
            fixture.host.state(&fixture.scope).unwrap()["state"],
            "waiting_for_user"
        );
        let job = fixture.host.jobs.lock().unwrap()[&fixture.scope.agent].clone();
        let before = job.budget.remaining().unwrap();
        assert!(job.budget.clock.lock().unwrap().human.contains(&job.run));
        // Cross the actual std::Instant run deadline while approval is pending.
        // The target is derived from this run, so slow setup neither shortens
        // the contract check nor forces an additional full-budget sleep.
        let after_deadline = job.budget.deadline + Duration::from_secs(1);
        while let Some(remaining) = after_deadline.checked_duration_since(Instant::now()) {
            tokio::time::sleep(remaining).await;
        }
        assert!(Instant::now() > job.budget.deadline);
        let remaining = job.budget.remaining().unwrap_or_else(|error| {
            panic!(
                "human wait exhausted the active budget: {error}: {}",
                fixture.host.state(&fixture.scope).unwrap()
            )
        });
        let tolerance = Duration::from_millis(100);
        assert!(
            remaining + tolerance >= before && remaining <= before + tolerance,
            "human waiting must preserve the active budget: before {before:?}, after {remaining:?}"
        );
        assert!(job.budget.clock.lock().unwrap().human.contains(&job.run));
        assert_eq!(
            fixture.host.state(&fixture.scope).unwrap()["state"],
            "waiting_for_user"
        );
        assert_eq!(fixture.model.calls.load(Ordering::Acquire), 1);
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
        fixture.answer(&pending, "once");
        assert_eq!(fixture.settle().await["state"], "completed");
        assert!(!job.budget.clock.lock().unwrap().human.contains(&job.run));
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 1);
        assert_eq!(fixture.model.calls.load(Ordering::Acquire), 2);
        assert_eq!(
            fixture
                .host
                .store
                .resolve_pending(
                    &fixture.scope,
                    "response",
                    pending["id"].as_str().unwrap(),
                    Some("once"),
                    None
                )
                .unwrap()["duplicate"],
            true
        );
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 1);
    }
    #[tokio::test]
    async fn deny_returns_to_model_and_same_action_does_not_prompt_again() {
        let fixture = Fixture::new(vec![("input_text", raw()), ("input_text", raw())]);
        fixture.start("root", 5, true);
        let pending = fixture.pending().await;
        fixture.answer(&pending, "deny");
        assert_eq!(fixture.settle().await["state"], "completed");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
        assert_eq!(fixture.model.calls.load(Ordering::Acquire), 3);
        assert!(
            fixture.host.store.pending(&fixture.scope, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn full_wakes_approval_but_questions_require_real_answer() {
        let fixture = Fixture::new(vec![
            ("input_text", raw()),
            (
                "ask_user",
                json!({"question":"Where next?","options":["A","B"]}),
            ),
        ]);
        fixture.start("root", 5, true);
        let first = fixture.pending().await;
        fixture
            .host
            .store
            .set_permissions(&fixture.scope, 0, None, Some(true))
            .unwrap();
        fixture.host.permissions_changed(&fixture.scope);
        for _ in 0..100 {
            if fixture.backend.writes.load(Ordering::Acquire) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let question = fixture.pending().await;
        assert_ne!(question["id"], first["id"]);
        assert_eq!(question["kind"], "question");
        assert_eq!(fixture.model.calls.load(Ordering::Acquire), 2);
        fixture
            .host
            .store
            .resolve_pending(
                &fixture.scope,
                "qresponse",
                question["id"].as_str().unwrap(),
                None,
                Some(json!("A")),
            )
            .unwrap();
        fixture.host.human_response_changed();
        assert_eq!(fixture.settle().await["state"], "completed");
    }
    #[tokio::test]
    async fn explicit_full_after_deny_allows_new_call_without_replaying_denied_action() {
        let fixture = Fixture::new(vec![
            ("input_text", raw()),
            ("ask_user", json!({"question":"Continue?"})),
            ("input_text", raw()),
        ]);
        fixture.start("root", 5, true);
        let denied = fixture.pending().await;
        fixture.answer(&denied, "deny");
        let question = fixture.pending().await;
        assert_eq!(question["kind"], "question");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
        fixture
            .host
            .set_permissions(&fixture.scope, 0, None, Some(true))
            .unwrap();
        fixture
            .host
            .store
            .resolve_pending(
                &fixture.scope,
                "question-response",
                question["id"].as_str().unwrap(),
                None,
                Some(json!("Continue")),
            )
            .unwrap();
        fixture.host.human_response_changed();
        assert_eq!(fixture.settle().await["state"], "completed");
        assert_eq!(
            fixture.backend.writes.load(Ordering::Acquire),
            1,
            "only the new full-authorized call executes"
        );
        assert_eq!(fixture.model.calls.load(Ordering::Acquire), 4);
        assert_eq!(
            fixture
                .host
                .store
                .poll_pending(&fixture.scope, denied["id"].as_str().unwrap())
                .unwrap()["response"]["decision"],
            "deny"
        );
    }
    #[tokio::test]
    async fn delegated_full_uses_root_permission_scope_without_polluting_session_conversation() {
        let fixture = Fixture::new(vec![("ask_user", json!({"question":"Wait for child"}))]);
        fixture
            .host
            .set_permissions(&fixture.scope, 0, None, Some(true))
            .unwrap();
        fixture.start("root", 5, true);
        fixture.pending().await;
        let job = fixture.host.jobs.lock().unwrap()[&fixture.scope.agent].clone();
        let context = ToolContext {
            history_unit_id: "delegate-unit".into(),
            vision: false,
            scope: fixture.scope.clone(),
            run_id: job.run.clone(),
            root_user_message_id: job.root.clone(),
            action_id: "delegate-action".into(),
            max_read_bytes: 4096,
            budget: job.budget.clone(),
            cancel: job.cancel.subscribe(),
            execution_gate: job.execution_gate.clone(),
            authorization_check: None,
        };
        let child = fixture
            .host
            .store
            .agent("owner", "desktop", Some("child-session"))
            .unwrap();
        let child_model = Arc::new(ModelStub {
            calls: AtomicU32::new(0),
            actions: vec![(
                "input_text".into(),
                json!({"text":"touch file","submit":true}),
            )],
        });
        let backend = fixture.backend.clone();
        fixture
            .host
            .delegate(
                &context,
                child.clone(),
                "delegated",
                "perform child work",
                json!({}),
                move || {
                    Ok(RunSnapshot {
                        revision: 1,
                        provider: Protocol::OpenaiChat,
                        builder: RequestBuilder {
                            settings: crate::model::RequestSettings {
                                model: "stub".into(),
                                temperature: None,
                                max_tokens: 2048,
                                additional_params: None,
                            },
                            system: "fixed".into(),
                            tools: terminal_tools(false),
                        },
                        model: child_model,
                        backend,
                        context_window: 128000,
                        max_rounds: 10,
                        max_seconds: 5,
                        allow_write: true,
                        vision: false,
                    })
                },
            )
            .unwrap();
        for _ in 0..100 {
            if fixture.host.state(&child).unwrap()["state"] == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(fixture.host.state(&child).unwrap()["state"], "completed");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 1);
        assert_eq!(
            fixture.host.store.permissions(&child).unwrap()["full_authorization"],
            false
        );
        assert!(
            fixture.host.store.pending(&child, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        fixture.host.cancel(&fixture.scope).unwrap();
        assert_eq!(fixture.settle().await["state"], "cancelled");
    }
    #[tokio::test]
    async fn old_v2_rule_never_auto_approves_v3_managed_native_action() {
        let args = json!({"program":"/usr/bin/tee","args":["-a","/tmp/marker"],"stdin":"marker","session_id":"terminal"});
        let fixture = Fixture::new(vec![("run_program", args.clone())]);
        fixture.start("root", 5, true);
        let pending = fixture.pending().await;
        let v3 = pending["fingerprint"].as_str().unwrap();
        assert!(v3.starts_with("v3:"));
        let old = format!("v2:{}", &v3[3..]);
        let old_row = fixture
            .host
            .store
            .create_pending(
                &fixture.scope,
                &fixture.scope,
                "old-run",
                "old-action",
                json!({"kind":"approval","fingerprint":old,"can_always":true,"tool":"run_command"}),
            )
            .unwrap();
        let old_id = old_row["id"].as_str().unwrap();
        fixture
            .host
            .store
            .resolve_pending(&fixture.scope, "old-response", old_id, Some("always"), None)
            .unwrap();
        fixture
            .host
            .store
            .consume_pending(&fixture.scope, old_id)
            .unwrap();
        assert!(
            fixture
                .host
                .store
                .rule_matches(&fixture.scope, &old)
                .unwrap()
        );
        assert!(!fixture.host.store.rule_matches(&fixture.scope, v3).unwrap());
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
        fixture.answer(&pending, "once");
        assert_eq!(fixture.settle().await["state"], "completed");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 1);
    }
    #[tokio::test]
    async fn cancelled_wait_never_executes_or_replays_old_pending() {
        let fixture = Fixture::new(vec![("input_text", raw())]);
        fixture.start("root", 5, true);
        let pending = fixture.pending().await;
        fixture.host.cancel(&fixture.scope).unwrap();
        assert_eq!(fixture.settle().await["state"], "cancelled");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
        assert!(
            fixture
                .host
                .store
                .resolve_pending(
                    &fixture.scope,
                    "late",
                    pending["id"].as_str().unwrap(),
                    Some("once"),
                    None
                )
                .is_err()
        );
    }
    #[tokio::test]
    async fn changed_cwd_after_approval_cannot_execute_old_action() {
        let fixture = Fixture::new(vec![("input_text", raw())]);
        fixture.start("root", 5, true);
        let pending = fixture.pending().await;
        *fixture.backend.cwd.lock().unwrap() = "/different".into();
        fixture.answer(&pending, "once");
        assert_eq!(fixture.settle().await["state"], "completed");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
    }
    #[tokio::test]
    async fn readonly_grant_does_not_create_approval_or_write() {
        let fixture = Fixture::new(vec![("input_text", raw())]);
        fixture.start("root", 5, false);
        assert_eq!(fixture.settle().await["state"], "completed");
        assert_eq!(fixture.backend.writes.load(Ordering::Acquire), 0);
        assert!(
            fixture.host.store.pending(&fixture.scope, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn shared_clock_pauses_only_with_all_runs_waiting_on_human_dependencies() {
        let scope = Scope {
            owner: "o".into(),
            desktop: "d".into(),
            agent: "a".into(),
            session: None,
        };
        let mut budget = Budget::new(1, 10, 10000, scope);
        budget.deadline = Instant::now() + Duration::from_millis(160);
        budget.change_active(true);
        let child = budget.waiting("child", true);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(budget.remaining().unwrap() < Duration::from_millis(120));
        let parent = budget.waiting("root", false);
        let before = budget.remaining().unwrap();
        budget
            .run_bounded(async {
                tokio::time::sleep(Duration::from_millis(220)).await;
                Ok(())
            })
            .await
            .unwrap();
        assert!(budget.remaining().unwrap() + Duration::from_millis(15) >= before);
        drop(parent);
        drop(child);
        tokio::time::sleep(Duration::from_millis(130)).await;
        assert!(budget.remaining().is_err());
    }
    #[test]
    fn permit_is_one_actual_effect_and_rechecks_at_commit() {
        let allow = Arc::new(AtomicBool::new(true));
        let flag = allow.clone();
        let permit = AuthorizationPermit {
            check: Arc::new(|| Ok(())),
            commit: Arc::new(move |_| {
                ensure!(flag.load(Ordering::Acquire), "authorization_revoked");
                Ok(())
            }),
            consumed: AtomicBool::new(false),
            command: None,
        };
        permit.check().unwrap();
        allow.store(false, Ordering::Release);
        assert!(permit.commit(None).is_err());
        allow.store(true, Ordering::Release);
        permit.commit(None).unwrap();
        assert!(permit.commit(None).is_err());
    }
}
