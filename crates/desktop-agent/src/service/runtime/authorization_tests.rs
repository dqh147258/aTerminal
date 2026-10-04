use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    host: Arc<Host>,
    scope: Scope,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("desktop");
        secure_dir(&state).unwrap();
        let store = Arc::new(
            ai_terminal_agent_runtime::store::Store::open(&state.join("agent.sqlite3")).unwrap(),
        );
        let host = Arc::new(Host {
            agents: ai_terminal_agent_runtime::host::AgentHost::new(
                store,
                tokio::runtime::Handle::current(),
            ),
            state_dir: state.clone(),
            account: crate::account::AccountManager::new(&state).unwrap(),
            config: crate::config::ConfigService::open(&state).unwrap(),
            assistant: crate::assistant::Assistant::default(),
            sessions: Mutex::new(HashMap::new()),
            session_order: Mutex::new(Vec::new()),
            recent_directories: Mutex::new(crate::recent_directories::RecentDirectories::new(
                &state,
            )),
            owners: Mutex::new(HashMap::new()),
            stop: Arc::new(AtomicBool::new(false)),
            workers: AtomicUsize::new(0),
        });
        let owner = host.account.owner();
        let desktop = host.config.snapshot(&owner).installation_id;
        let scope = host.agents.store.agent(&owner, &desktop, None).unwrap();
        Self {
            _dir: dir,
            host,
            scope,
        }
    }
    fn call(&self, args: Value) -> Result<Value> {
        let mut args = args;
        args["version"] = json!(1);
        args["agent_id"] = json!(self.scope.agent);
        let reply = dispatch(
            &self.host,
            Request {
                operation: Operation::Agent as i32,
                client: 1,
                text: args.to_string(),
                ..Default::default()
            },
        )?;
        Ok(serde_json::from_str(&reply.history[0])?)
    }
}
#[tokio::test]
async fn user_permissions_rpc_is_cas_bound_and_available_without_terminal() {
    let f = Fixture::new();
    let first = f.call(json!({"action":"permissions"})).unwrap();
    assert_eq!(first["permission_mode"], "ask");
    assert_eq!(first["full_authorization"], false);
    assert_eq!(first["can_mutate"], true);
    assert_eq!(
        f.call(json!({"action":"set_permissions","expected_revision":0,"full_authorization":true}))
            .unwrap()["revision"],
        1
    );
    assert_eq!(
        f.call(
            json!({"action":"set_permissions","expected_revision":0,"full_authorization":false})
        )
        .unwrap_err()
        .to_string(),
        "permission_revision_conflict"
    );
    let state = f.call(json!({"action":"state"})).unwrap();
    assert_eq!(state["permissions"]["full_authorization"], true);
    assert!(state["pending"]["items"].is_array());
    assert_eq!(
        f.call(
            json!({"action":"set_permissions","expected_revision":1,"permission_mode":"read_only"})
        )
        .unwrap()["full_authorization"],
        false
    );
}
#[tokio::test]
async fn legacy_requests_initialize_ask_or_readonly_without_full_upgrade() {
    for allow in [false, true] {
        let f = Fixture::new();
        let error = f
            .call(json!({"action":"send","request_id":"old","message":"hello","allow_input":allow}))
            .unwrap_err();
        assert_eq!(error.to_string(), "model_not_configured");
        let permission = f.call(json!({"action":"permissions"})).unwrap();
        assert_eq!(
            permission["permission_mode"],
            if allow { "ask" } else { "read_only" }
        );
        assert_eq!(permission["full_authorization"], false);
    }
}
#[tokio::test]
async fn rpc_resolves_only_exact_scope_and_uses_long_details_ack() {
    let f = Fixture::new();
    let item=f.host.agents.store.create_pending(&f.scope,&f.scope,"run","a",json!({"kind":"approval","fingerprint":"exact","requires_details":true,"can_always":true,"_details":{"command":"/usr/bin/printf target"}})).unwrap();
    let id = item["id"].as_str().unwrap();
    assert_eq!(
        f.call(json!({"action":"pending"})).unwrap()["items"][0]["id"],
        id
    );
    assert!(
        f.call(json!({"action":"resolve","request_id":"r","pending_id":id,"decision":"once"}))
            .is_err()
    );
    let details = f
        .call(json!({"action":"approval_details","pending_id":id}))
        .unwrap();
    assert!(
        details["text"]
            .as_str()
            .unwrap()
            .contains("/usr/bin/printf target")
    );
    let resolved=f.call(json!({"action":"resolve","request_id":"r","pending_id":id,"decision":"always","details_ack":true,"fingerprint":"exact"})).unwrap();
    assert_eq!(resolved["duplicate"], false);
    assert_eq!(f.call(json!({"action":"resolve","request_id":"r","pending_id":id,"decision":"always","details_ack":true,"fingerprint":"exact"})).unwrap()["duplicate"],true);
    f.host.agents.store.consume_pending(&f.scope, id).unwrap();
    let rule = f.call(json!({"action":"rules"})).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    f.call(json!({"action":"revoke_rule","request_id":"revoke","rule_id":rule}))
        .unwrap();
    assert_eq!(
        f.call(json!({"action":"rules"})).unwrap()["items"],
        json!([])
    );
    assert!(f.call(json!({"action":"resolve","request_id":"foreign","pending_id":"unknown","decision":"once"})).is_err());
    let mut args = json!({"version":1,"agent_id":f.scope.agent,"action":"permissions"});
    let error = dispatch(
        &f.host,
        Request {
            operation: Operation::Agent as i32,
            client: 1,
            session: "other-session".into(),
            text: args.to_string(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "agent_session_scope_mismatch");
    args["origin"] = json!("user");
    assert!(
        dispatch(
            &f.host,
            Request {
                text: args.to_string(),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_actor_commit_rejects_another_agents_draft_after_broker_preflight() {
    use ai_terminal_agent_runtime::model::{Protocol, RequestSettings};
    use axum::response::IntoResponse;
    use std::sync::atomic::AtomicU32;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicU32::new(0));
    let requests_count = calls.clone();
    let router=axum::Router::new().route("/chat/completions",axum::routing::post(move || {
        let count=requests_count.clone();async move {
            let first=count.fetch_add(1,Ordering::AcqRel)==0;
            let delta=if first {json!({"tool_calls":[{"index":0,"id":"approved","type":"function","function":{"name":"run_command","arguments":json!({"command":"/usr/bin/printf APPROVED"}).to_string()}}]})} else {json!({"content":"finished"})};
            let chunk=json!({"id":"stub","object":"chat.completion.chunk","created":0,"model":"stub","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let end=json!({"id":"stub","object":"chat.completion.chunk","created":0,"model":"stub","choices":[{"index":0,"delta":{},"finish_reason":if first {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
            ([("content-type","text/event-stream")],format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n")).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let f = Fixture::new();
    let owner = f.host.account.owner();
    let provider = Provider {
        id: "test".into(),
        name: "local stub".into(),
        connection: ai_terminal_agent_runtime::model::Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: format!("http://{address}"),
            api_version: None,
        },
        catalog_url: None,
        secret_ref: None,
        credential_revision: 0,
        enabled: true,
    };
    let mut config = OwnerConfig::default();
    config
        .providers
        .insert(provider.id.clone(), provider.clone());
    f.host
        .config
        .execute(
            &owner,
            crate::config::Command::Replace {
                expected_revision: f.host.config.snapshot(&owner).revision,
                config,
                secrets: Default::default(),
            },
        )
        .unwrap();
    // Explicit /bin/sh source avoids startup files; this isolated PTY never runs a model or user command.
    let created = super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Create as i32,
            client: 1,
            cwd: f._dir.path().to_string_lossy().into_owned(),
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf READY; while IFS= read -r line; do printf 'LINE:%s\\n' \"$line\"; done"
                    .into(),
            ],
            rows: 24,
            cols: 80,
            ..Default::default()
        },
    )
    .unwrap()
    .info
    .unwrap();
    let real = f.host.sessions.lock().unwrap()[&created.id].clone();
    let attached = request_actor(
        &real,
        Request {
            operation: Operation::AttachDesktop as i32,
            client: 1,
            session_epoch: created.epoch,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(attached.error.is_empty());
    let (proxy, requests) = mpsc::sync_channel::<ActorMessage>(32);
    let target = real.clone();
    let session_id = created.id.clone();
    let injected = Arc::new(AtomicBool::new(false));
    let signal = injected.clone();
    let shim = std::thread::spawn(move || {
        while let Ok(message) = requests.recv() {
            if message.request.operation == Operation::AgentWrite as i32
                && message.authorization.is_some()
                && !signal.swap(true, Ordering::AcqRel)
            {
                // The original message has already passed every Broker preflight and is queued.
                // A separately authorized writer inserts a draft before the real Actor handles it.
                let info = request_actor(
                    &target,
                    Request {
                        operation: Operation::Poll as i32,
                        client: 700,
                        ..Default::default()
                    },
                )
                .unwrap()
                .info
                .unwrap();
                let acquire = request_actor_guarded(
                    &target,
                    Request {
                        operation: Operation::AgentAcquire as i32,
                        client: 700,
                        session_epoch: info.epoch,
                        manual_revision: info.manual_revision,
                        ..Default::default()
                    },
                    Some(Arc::new(Mutex::new(true))),
                )
                .unwrap();
                let epoch = acquire.info.unwrap().control_epoch;
                let result = request_actor_guarded(
                    &target,
                    Request {
                        operation: Operation::AgentWrite as i32,
                        client: 700,
                        session: session_id.clone(),
                        session_epoch: info.epoch,
                        manual_revision: info.manual_revision,
                        control_epoch: epoch,
                        input_kind: 1,
                        text: "unrelated draft".into(),
                        submit: false,
                        ..Default::default()
                    },
                    Some(Arc::new(Mutex::new(true))),
                )
                .unwrap();
                assert!(result.error.is_empty(), "{}", result.error);
            }
            if target.send(message).is_err() {
                break;
            }
        }
    });
    f.host
        .sessions
        .lock()
        .unwrap()
        .insert(created.id.clone(), proxy);
    let view = f.host.config.snapshot(&owner);
    let scope = f
        .host
        .agents
        .store
        .agent(&owner, &view.installation_id, Some(&created.id))
        .unwrap();
    let model =
        ai_terminal_agent_runtime::model::connect(&provider.connection, "stub", "").unwrap();
    let backend = Backend::new(
        &f.host,
        scope.clone(),
        String::new(),
        Arc::new(view.config),
        view.revision,
        provider,
        None,
        None,
    )
    .unwrap();

    let engine_model = model.clone();
    let broker = backend.clone();
    f.host
        .agents
        .submit(
            scope.clone(),
            "root",
            "perform command",
            json!({}),
            true,
            "",
            move || {
                Ok(RunSnapshot {
                    revision: 1,
                    provider: Protocol::OpenaiChat,
                    builder: RequestBuilder {
                        settings: RequestSettings {
                            model: "stub".into(),
                            temperature: None,
                            max_tokens: 2048,
                            additional_params: None,
                        },
                        system: "fixed".into(),
                        tools: terminal_tools(false),
                    },
                    model: engine_model,
                    backend: broker,
                    context_window: 128000,
                    max_rounds: 4,
                    max_seconds: 30,
                    allow_write: true,
                    vision: false,
                })
            },
        )
        .unwrap();
    let mut pending = None;
    for _ in 0..300 {
        let page = f.host.agents.store.pending(&scope, None).unwrap();
        pending = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["state"] == "pending")
            .cloned();
        if pending.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let pending = pending.expect("real Broker approval");
    f.host
        .agents
        .store
        .resolve_pending(
            &scope,
            "answer",
            pending["id"].as_str().unwrap(),
            Some("once"),
            None,
        )
        .unwrap();
    f.host.agents.human_response_changed();
    let mut result = Value::Null;
    for _ in 0..500 {
        result = f.host.agents.state(&scope).unwrap();
        if !matches!(
            result["state"].as_str(),
            Some("running" | "waiting_for_user" | "finishing" | "stopping")
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(result["state"], "completed", "{result}");
    assert!(injected.load(Ordering::Acquire));
    let info = request_actor(
        &real,
        Request {
            operation: Operation::Poll as i32,
            client: 1,
            ..Default::default()
        },
    )
    .unwrap()
    .info
    .unwrap();
    let shell: Value = serde_json::from_str(&info.shell_status).unwrap();
    assert_eq!(
        shell["host_input_boundary"]["input_revision"], 1,
        "refused action never reached PTY writer"
    );
    let history = f.host.agents.store.history(&scope, None).unwrap();
    let refused = history
        .items
        .iter()
        .flat_map(|item| item.value["updates"].as_array().into_iter().flatten())
        .filter_map(|update| update["result_record_id"].as_str())
        .any(|record| {
            f.host
                .agents
                .store
                .record_page(&scope, record, "body", None, 12288)
                .unwrap()["body"]
                .as_str()
                .unwrap_or("")
                .contains("authorization_input_changed")
        });
    assert!(
        refused,
        "Actor rejection was archived as a model tool result"
    );
    // Close only the isolated test terminal and release the proxy thread.
    let closed = super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: created.id,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(closed.error.is_empty());
    drop(backend);
    drop(f);
    shim.join().unwrap();
    server.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_default_cwd_and_native_observation_follow_real_session_directory_and_revoke_stale_cwd()
 {
    use ai_terminal_agent_runtime::{
        host::Budget,
        model::{Connection, Protocol},
    };
    let f = Fixture::new();
    let owner = f.host.account.owner();
    let provider = Provider {
        id: "test".into(),
        name: "local stub".into(),
        connection: Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: "http://127.0.0.1:1".into(),
            api_version: None,
        },
        catalog_url: None,
        secret_ref: None,
        credential_revision: 0,
        enabled: true,
    };
    let mut config = OwnerConfig::default();
    config
        .providers
        .insert(provider.id.clone(), provider.clone());
    f.host
        .config
        .execute(
            &owner,
            crate::config::Command::Replace {
                expected_revision: 0,
                config,
                secrets: Default::default(),
            },
        )
        .unwrap();
    let files=std::collections::BTreeMap::from([
        ("SKILL.md".into(),STANDARD.encode("---\nname: cwd-check\ndescription: isolated directory test\n---\nRead the working directory.")),
        ("scripts/cwd.sh".into(),STANDARD.encode("pwd\n"))]);
    f.host
        .config
        .execute(
            &owner,
            crate::config::Command::SkillFiles {
                id: "cwd-check".into(),
                files,
                expected_revision: 1,
            },
        )
        .unwrap();
    let next = f._dir.path().join("next");
    std::fs::create_dir(&next).unwrap();
    let created = super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Create as i32,
            client: 1,
            cwd: f._dir.path().to_string_lossy().into_owned(),
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf READY; read -r line; cd -- \"$1\"; printf CHANGED; exec cat".into(),
                "cwd-test".into(),
                next.to_string_lossy().into_owned(),
            ],
            rows: 24,
            cols: 80,
            ..Default::default()
        },
    )
    .unwrap()
    .info
    .unwrap();
    let actor = f.host.sessions.lock().unwrap()[&created.id].clone();
    request_actor(
        &actor,
        Request {
            operation: Operation::AttachDesktop as i32,
            client: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let view = f.host.config.snapshot(&owner);
    let scope = f
        .host
        .agents
        .store
        .agent(&owner, &view.installation_id, Some(&created.id))
        .unwrap();
    let backend = Backend::new(
        &f.host,
        scope.clone(),
        String::new(),
        Arc::new(view.config),
        view.revision,
        provider,
        None,
        None,
    )
    .unwrap();
    let root = f
        .host
        .agents
        .store
        .accept_user(&scope, "r", "test", json!({}))
        .unwrap();
    let (_cancel, receiver) = tokio::sync::watch::channel(false);
    let context = ToolContext {
        history_unit_id: root.user_message_id,
        vision: false,
        scope: scope.clone(),
        run_id: root.run_id,
        root_user_message_id: root.root_user_message_id,
        action_id: "change-cwd".into(),
        max_read_bytes: 4096,
        budget: Arc::new(Budget::new(30, 10, 10000, scope.clone())),
        cancel: receiver,
        execution_gate: Arc::new(Mutex::new(true)),
        authorization_check: None,
    };
    let args = json!({"skill_id":"user/cwd-check","action":"script","arguments":{"path":"scripts/cwd.sh","interpreter":"sh"}});
    let before = backend
        .action_descriptor(&context, "skill_action", &args)
        .unwrap();
    assert_eq!(
        std::fs::canonicalize(before.cwd.unwrap()).unwrap(),
        f._dir.path().canonicalize().unwrap()
    );
    let old_info = backend.info(&created.id).unwrap().info.unwrap();
    let old_cwd = crate::process::cwd(&old_info).unwrap();
    let observation = backend
        .inspection_context(&context, &created.id, &old_cwd)
        .unwrap();
    backend
        .write(
            &context,
            &created.id,
            Request {
                operation: Operation::AgentWrite as i32,
                input_kind: 1,
                text: "change".into(),
                submit: true,
                ..Default::default()
            },
        )
        .unwrap();
    let next = next.canonicalize().unwrap();
    for _ in 0..100 {
        let info = backend.info(&created.id).unwrap().info.unwrap();
        if crate::process::cwd(&info).as_ref() == Some(&next) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        observation.commit_authorization(None).is_err(),
        "native spawn refuses a cached directory"
    );
    let after = backend
        .action_descriptor(&context, "skill_action", &args)
        .unwrap();
    assert_eq!(std::fs::canonicalize(after.cwd.unwrap()).unwrap(), next);
    let output = backend
        .invoke(context.clone(), "skill_action", args)
        .await
        .unwrap();
    assert_eq!(output.value["exit_code"], 0);
    assert_eq!(
        std::fs::canonicalize(output.observation.unwrap().body.trim()).unwrap(),
        next
    );
    backend.finished();
    super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: created.id,
            ..Default::default()
        },
    )
    .unwrap();
}

#[cfg(unix)]
fn native_fixture() -> (
    Fixture,
    Arc<Backend>,
    Scope,
    ToolContext,
    tokio::sync::watch::Sender<bool>,
    String,
) {
    use ai_terminal_agent_runtime::{
        host::Budget,
        model::{Connection, Protocol},
    };
    let f = Fixture::new();
    let owner = f.host.account.owner();
    let provider = Provider {
        id: "native".into(),
        name: "native fixture".into(),
        connection: Connection {
            protocol: Protocol::OpenaiChat,
            endpoint: "http://127.0.0.1:1".into(),
            api_version: None,
        },
        catalog_url: None,
        secret_ref: None,
        credential_revision: 0,
        enabled: true,
    };
    let mut config = OwnerConfig::default();
    config
        .providers
        .insert(provider.id.clone(), provider.clone());
    f.host
        .config
        .execute(
            &owner,
            crate::config::Command::Replace {
                expected_revision: 0,
                config,
                secrets: Default::default(),
            },
        )
        .unwrap();
    let terminal = super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Create as i32,
            client: 1,
            cwd: f._dir.path().to_string_lossy().into_owned(),
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf READY; exec cat".into(),
            ],
            rows: 24,
            cols: 80,
            ..Default::default()
        },
    )
    .unwrap()
    .info
    .unwrap();
    let actor = f.host.sessions.lock().unwrap()[&terminal.id].clone();
    request_actor(
        &actor,
        Request {
            operation: Operation::AttachDesktop as i32,
            client: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let view = f.host.config.snapshot(&owner);
    let scope = f
        .host
        .agents
        .store
        .agent(&owner, &view.installation_id, Some(&terminal.id))
        .unwrap();
    let backend = Backend::new(
        &f.host,
        scope.clone(),
        String::new(),
        Arc::new(view.config),
        view.revision,
        provider,
        None,
        None,
    )
    .unwrap();
    let root = f
        .host
        .agents
        .store
        .accept_user(&scope, "root", "fixture controlled invocation", json!({}))
        .unwrap();
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let context = ToolContext {
        history_unit_id: root.user_message_id,
        vision: false,
        scope: scope.clone(),
        run_id: root.run_id,
        root_user_message_id: root.root_user_message_id,
        action_id: "native-action".into(),
        max_read_bytes: 4096,
        budget: Arc::new(Budget::new(30, 10, 10000, scope.clone())),
        cancel: receiver,
        execution_gate: Arc::new(Mutex::new(true)),
        authorization_check: None,
    };
    (f, backend, scope, context, cancel, terminal.id)
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_unknown_program_is_once_full_only_and_returns_real_nonzero_eof_and_bounded_output()
{
    let (f, backend, scope, mut context, _cancel, terminal) = native_fixture();
    let args = json!({"program":"/bin/sh","args":["-c","printf OUT; printf ERR >&2; exit 7"]});
    let descriptor = backend
        .action_descriptor(&context, "run_program", &args)
        .unwrap();
    assert!(!ai_terminal_agent_runtime::authorization::assess(&descriptor).can_always);
    let output = backend
        .invoke(context.clone(), "run_program", args)
        .await
        .unwrap();
    assert_eq!(output.value["exit_code"], 7);
    assert_eq!(output.value["source"], "native_program");
    let command = output.value["command_id"].as_str().unwrap();
    assert_eq!(
        f.host.agents.store.command(&scope, command).unwrap()["exit_code"],
        7
    );
    context.action_id = "native-eof".into();
    let output = backend
        .invoke(
            context.clone(),
            "run_program",
            json!({"program":"/bin/cat","args":[]}),
        )
        .await
        .unwrap();
    assert_eq!(output.value["exit_code"], 0);
    assert_eq!(
        serde_json::from_str::<Value>(&output.observation.unwrap().body).unwrap()["stdout"]["text"],
        ""
    );
    context.action_id = "native-limit".into();
    let output = backend
        .invoke(
            context,
            "run_program",
            json!({"program":"/bin/sh","args":["-c","head -c 90000 /dev/zero"]}),
        )
        .await
        .unwrap();
    assert_eq!(output.value["stdout_truncated"], true);
    let body: Value = serde_json::from_str(&output.observation.unwrap().body).unwrap();
    assert_eq!(body["stdout"]["text"].as_str().unwrap().len(), 65536);
    backend.finished();
    super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: terminal,
            ..Default::default()
        },
    )
    .unwrap();
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capabilities_remain_readable_for_a_known_closed_session() {
    let (f, backend, _scope, context, _cancel, terminal) = native_fixture();
    super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: terminal,
            ..Default::default()
        },
    )
    .unwrap();
    let output = backend
        .invoke(context, "get_capabilities", json!({}))
        .await
        .unwrap();
    assert_eq!(output.value["role"], "session");
    assert_eq!(output.value["terminal"]["available"], false);
    assert!(
        output.value["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "run_program")
    );
    assert_eq!(output.value["native_program"]["pty_permanent_rules"], false);
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_large_stdout_and_stderr_do_not_interrupt_later_effects() {
    let (f, backend, _scope, context, _cancel, terminal) = native_fixture();
    let marker = f._dir.path().join("after-large-output");
    let script = format!(
        "/usr/bin/head -c 524288 /dev/zero; /usr/bin/head -c 524288 /dev/zero >&2; /usr/bin/touch '{}'",
        marker.display()
    );
    let output = backend
        .invoke(
            context,
            "run_program",
            json!({"program":"/bin/sh","args":["-e","-c",script]}),
        )
        .await
        .unwrap();
    assert_eq!(output.value["exit_code"], 0);
    assert_eq!(output.value["stdout_truncated"], true);
    assert_eq!(output.value["stderr_truncated"], true);
    assert!(marker.exists());
    let body: Value = serde_json::from_str(&output.observation.unwrap().body).unwrap();
    assert_eq!(body["stdout"]["text"].as_str().unwrap().len(), 65536);
    assert_eq!(body["stderr"]["text"].as_str().unwrap().len(), 65536);
    backend.finished();
    super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: terminal,
            ..Default::default()
        },
    )
    .unwrap();
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_cancellation_kills_own_process_group_and_retains_unknown_without_touching_pty() {
    let (f, backend, scope, context, cancel, terminal) = native_fixture();
    let marker = f._dir.path().join("must-not-run");
    let script = format!(
        "sleep 0.3; /usr/bin/touch '{}'; sleep 30",
        marker.to_string_lossy()
    );
    let runner = backend.clone();
    let run = tokio::spawn(async move {
        runner
            .invoke(
                context,
                "run_program",
                json!({"program":"/bin/sh","args":["-c",script]}),
            )
            .await
    });
    let mut command = None;
    for _ in 0..100 {
        command = f
            .host
            .agents
            .store
            .history(&scope, None)
            .unwrap()
            .items
            .iter()
            .find(|row| row.kind == "command_submission")
            .and_then(|row| row.value["command_id"].as_str())
            .map(str::to_owned);
        if let Some(id) = &command
            && f.host.agents.store.command(&scope, id).unwrap()["accepted"] == true
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let command = command.expect("native command prepared");
    cancel.send(true).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(
        !marker.exists(),
        "cancelled native group cannot continue delayed mutation"
    );
    let result = f.host.agents.store.command(&scope, &command).unwrap();
    assert_eq!(result["state"], "unknown");
    assert_eq!(result["final"], true);
    assert!(result["exit_code"].is_null());
    assert!(
        !backend.info(&terminal).unwrap().info.unwrap().exited,
        "the existing PTY process remains alive"
    );
    backend.finished();
    super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: terminal,
            ..Default::default()
        },
    )
    .unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_agent_reads_and_waits_running_native_while_pty_input_cannot_invalidate_it() {
    use ai_terminal_agent_runtime::host::Budget;
    let (f, backend, scope, context, _cancel, terminal) = native_fixture();
    let marker = f._dir.path().join("native-completed");
    let script = format!(
        "sleep 0.6; /usr/bin/printf NATIVE > '{}'",
        marker.to_string_lossy()
    );
    let runner = backend.clone();
    let run = tokio::spawn(async move {
        runner
            .invoke(
                context,
                "run_program",
                json!({"program":"/bin/sh","args":["-c",script]}),
            )
            .await
    });
    let mut id = None;
    for _ in 0..100 {
        id = f
            .host
            .agents
            .store
            .history(&scope, None)
            .unwrap()
            .items
            .iter()
            .find(|row| row.kind == "command_submission")
            .and_then(|row| row.value["command_id"].as_str())
            .map(str::to_owned);
        if let Some(id) = &id
            && f.host.agents.store.command(&scope, id).unwrap()["accepted"] == true
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let id = id.expect("native started");
    let observer = f
        .host
        .agents
        .store
        .create_global(&scope.owner, &scope.desktop, "observer")
        .unwrap();
    let root = f
        .host
        .agents
        .store
        .accept_user(
            &observer,
            "observe",
            "read and write isolated terminal",
            json!({}),
        )
        .unwrap();
    let view = f.host.config.snapshot(&scope.owner);
    let provider = view.config.providers["native"].clone();
    let observer_backend = Backend::new(
        &f.host,
        observer.clone(),
        String::new(),
        Arc::new(view.config),
        view.revision,
        provider,
        None,
        None,
    )
    .unwrap();
    let (_cancel, receiver) = tokio::sync::watch::channel(false);
    let mut ctx = ToolContext {
        history_unit_id: root.user_message_id,
        vision: false,
        scope: observer.clone(),
        run_id: root.run_id,
        root_user_message_id: root.root_user_message_id,
        action_id: "observe-native".into(),
        max_read_bytes: 4096,
        budget: Arc::new(Budget::new(30, 10, 10000, observer)),
        cancel: receiver,
        execution_gate: Arc::new(Mutex::new(true)),
        authorization_check: None,
    };
    let running = observer_backend
        .invoke(ctx.clone(), "get_command_result", json!({"command_id":id}))
        .await
        .unwrap();
    assert_eq!(running.value["state"], "running");
    assert_eq!(running.value["final"], false);
    ctx.action_id = "other-agent-pty".into();
    let written = observer_backend
        .invoke(
            ctx.clone(),
            "input_text",
            json!({"session_id":terminal,"text":"independent draft","submit":false}),
        )
        .await
        .unwrap();
    assert_eq!(written.value["accepted"], true);
    assert_eq!(
        f.host.agents.store.command(&scope, &id).unwrap()["state"],
        "running"
    );
    let waited = observer_backend
        .invoke(
            ctx,
            "wait_command",
            json!({"command_id":id,"timeout_ms":2000}),
        )
        .await
        .unwrap();
    assert_eq!(waited.value["state"], "completed");
    assert_eq!(waited.value["exit_code"], 0);
    assert_eq!(waited.value["timed_out"], false);
    assert!(waited.value["result_record_id"].is_string());
    assert_eq!(waited.value["stdout"]["text"], "");
    assert_eq!(
        waited.value["output_record"]["record_id"],
        waited.value["result_record_id"]
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "NATIVE");
    run.await.unwrap().unwrap();
    observer_backend.finished();
    backend.finished();
    super::super::dispatch(
        &f.host,
        Request {
            operation: Operation::Close as i32,
            client: 1,
            session: terminal,
            ..Default::default()
        },
    )
    .unwrap();
}
