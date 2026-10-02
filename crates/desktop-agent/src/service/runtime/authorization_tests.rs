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
