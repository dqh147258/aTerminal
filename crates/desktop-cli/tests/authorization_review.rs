//! Independent acceptance against a disposable account, encrypted mobile RPC,
//! a deterministic local model and real PTY files. Build account_demo first.
#![cfg(unix)]
use ai_terminal_agent::Client;
use ai_terminal_mobile::{Account, RemoteTerminal};
use ai_terminal_protocol::local::{Operation, Request};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

struct Fixture {
    child: Child,
    dir: PathBuf,
    config: Value,
    local: Client,
    successful: bool,
}
impl Fixture {
    async fn start() -> Result<Self> {
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_aTerminal"));
        let example = executable
            .parent()
            .context("CLI directory unavailable")?
            .join("examples/account_demo");
        ensure!(
            example.is_file(),
            "Build account_demo before this acceptance test"
        );
        let dir = std::env::temp_dir().join(format!(
            "aterminal-authorization-review-{:x}",
            ai_terminal_agent::random_id()
        ));
        let mut builder = fs::DirBuilder::new();
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700).create(&dir)?;
        let log = fs::File::create(dir.join("fixture.log"))?;
        let mut child = Command::new(example)
            .arg(&dir)
            .arg(executable)
            .arg("--authorization-test")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !dir.join("account-fixture.json").exists() {
            if let Some(status) = child.try_wait()? {
                bail!("Fixture exited {status}; inspect {}", dir.display());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Fixture startup timed out; inspect {}", dir.display());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let config = serde_json::from_slice(&fs::read(dir.join("account-fixture.json"))?)?;
        let local = Client::connect(&dir)?;
        Ok(Self {
            child,
            dir,
            config,
            local,
            successful: false,
        })
    }
    async fn phone(&self) -> Result<Phone> {
        let config = self.config.clone();
        tokio::task::spawn_blocking(move || {
            let account = Arc::new(Account::new());
            account.login(
                config["server"].as_str().unwrap().into(),
                config["username"].as_str().unwrap().into(),
                config["password"].as_str().unwrap().into(),
                "Independent review phone".into(),
                "ios".into(),
                String::new(),
            )?;
            let deadline = Instant::now() + Duration::from_secs(15);
            let desktop = loop {
                if let Some(device) = account
                    .devices()?
                    .into_iter()
                    .find(|device| device.platform == "desktop" && device.online)
                {
                    break device.id;
                }
                ensure!(Instant::now() < deadline, "Temporary Desktop unavailable");
                std::thread::sleep(Duration::from_millis(100));
            };
            let remote = Arc::new(RemoteTerminal::new());
            account.connect(desktop, remote.clone())?;
            remote.select(config["session"].as_str().unwrap().into(), false)?;
            Ok(Phone {
                account,
                remote,
                session: config["session"].as_str().unwrap().into(),
            })
        })
        .await?
    }
    fn marker(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.dir.join(name)).ok()
    }
    async fn wait_marker(&self, name: &str, expected: &str) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.marker(name).as_deref() == Some(expected) {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "PTY marker {name} mismatch: {:?}",
                self.marker(name)
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    fn calls(&self, id: &str) -> usize {
        fs::read_to_string(self.dir.join("authorization-model-observations.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|value| value["id"] == id)
            .count()
    }
    fn results(&self, id: &str) -> Result<Vec<Value>> {
        Ok(serde_json::from_slice(&fs::read(
            self.dir.join(format!("authorization-results-{id}.json")),
        )?)?)
    }
    fn manual_input(&self, text: &str, submit: bool) -> Result<()> {
        let session = self.config["session"].as_str().unwrap();
        let info = self
            .local
            .call(Request {
                operation: Operation::Acquire as i32,
                session: session.into(),
                ..Default::default()
            })?
            .info
            .context("Cannot acquire isolated terminal")?;
        self.local.call(Request {
            operation: Operation::Input as i32,
            session: session.into(),
            session_epoch: info.epoch,
            control_epoch: info.control_epoch,
            input_seq: info.next_input_seq,
            input_kind: 1,
            text: text.into(),
            submit,
            ..Default::default()
        })?;
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if let Some(output) = std::env::var_os("AUTH_REVIEW_EVIDENCE_DIR") {
            let output = PathBuf::from(output).join(self.dir.file_name().unwrap());
            fs::create_dir_all(&output)?;
            for entry in fs::read_dir(&self.dir)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                // Only synthetic model observations/markers are exported, never
                // fixture account credentials, endpoint tokens or database files.
                if name.starts_with("authorization-")
                    || name.starts_with("auth-review-") && entry.path().is_file()
                {
                    fs::copy(entry.path(), output.join(name.as_ref()))?;
                }
            }
            fs::write(
                output.join("acceptance.json"),
                serde_json::to_vec_pretty(
                    &json!({"passed":true,"real_encrypted_rpc":true,"model":"local deterministic fixture"}),
                )?,
            )?;
        }
        self.successful = true;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.local.call(Request {
            operation: Operation::Shutdown as i32,
            ..Default::default()
        });
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.successful || std::thread::panicking() {
            eprintln!(
                "Preserved failed isolated fixture evidence: {}",
                self.dir.display()
            );
        } else {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}
struct Phone {
    account: Arc<Account>,
    remote: Arc<RemoteTerminal>,
    session: String,
}
impl Phone {
    async fn rpc(&self, mut value: Value) -> Result<Value> {
        value["version"] = json!(1);
        let remote = self.remote.clone();
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let response = remote.agent(session, value.to_string())?;
            Ok(serde_json::from_str(&response)?)
        })
        .await?
    }
    async fn send(&self, id: &str, steps: Value, legacy: Option<bool>) -> Result<Value> {
        let scenario = json!({"id":id,"steps":steps});
        let mut request = json!({"action":"send","request_id":format!("review-{id}"),"message":format!("AUTH_REVIEW:{scenario}")});
        if let Some(allow) = legacy {
            request["allow_input"] = json!(allow);
        } else {
            request["permission_mode"] = json!("ask");
        }
        self.rpc(request).await
    }
    async fn pending(&self, kind: &str) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let page = self.rpc(json!({"action":"pending"})).await?;
            if let Some(item) = page["items"]
                .as_array()
                .context("pending items missing")?
                .iter()
                .find(|item| {
                    item["kind"] == kind
                        && matches!(
                            item["state"].as_str(),
                            Some("pending" | "waiting" | "waiting_for_user")
                        )
                })
            {
                return Ok(item.clone());
            }
            let state = self.rpc(json!({"action":"state"})).await?;
            ensure!(
                state["state"] != "paused",
                "Run paused before pending: {state}"
            );
            ensure!(
                Instant::now() < deadline,
                "Expected {kind} request missing: {state}"
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
    async fn resolve(&self, pending: &Value, request: &str, decision: &str) -> Result<Value> {
        let mut command = json!({"action":"resolve","request_id":request,"pending_id":pending["id"],"decision":decision});
        if decision != "deny" && pending["requires_details"] == true {
            let mut cursor = Value::Null;
            let mut complete = String::new();
            let mut seen = std::collections::HashSet::new();
            loop {
                let page = self.rpc(json!({"action":"approval_details","pending_id":pending["id"],"cursor":cursor})).await?;
                ensure!(
                    page["pending_id"] == pending["id"]
                        && page["fingerprint"] == pending["fingerprint"],
                    "Approval details changed target"
                );
                ensure!(
                    page["truncated"] == false,
                    "Approval detail page was truncated"
                );
                complete.push_str(
                    page["text"]
                        .as_str()
                        .context("Approval details text missing")?,
                );
                if page["has_more"] != true {
                    break;
                }
                cursor = page["cursor"].clone();
                ensure!(
                    cursor.is_string() && seen.insert(cursor.to_string()) && seen.len() <= 256,
                    "Invalid approval detail cursor"
                );
            }
            let _: Value = serde_json::from_str(&complete)
                .context("Approval details were not complete JSON")?;
            command["fingerprint"] = pending["fingerprint"].clone();
            command["details_ack"] = json!(true);
        }
        self.rpc(command).await
    }
    async fn full(&self, enabled: bool) -> Result<Value> {
        let mode = self.rpc(json!({"action":"permissions"})).await?;
        self.rpc(json!({"action":"set_permissions","expected_revision":mode["revision"],"full_authorization":enabled})).await
    }
    async fn settled(&self) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let state = self.rpc(json!({"action":"state"})).await?;
            if !matches!(
                state["state"].as_str(),
                Some("running" | "waiting" | "waiting_for_user" | "stopping" | "finishing")
            ) {
                return Ok(state);
            }
            ensure!(Instant::now() < deadline, "Run failed to settle: {state}");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}
impl Drop for Phone {
    fn drop(&mut self) {
        let remote = self.remote.clone();
        let account = self.account.clone();
        // Mobile FFI owns a Tokio runtime and must run outside an async worker.
        let _ = std::thread::spawn(move || {
            let _ = remote.disconnect();
            let _ = account.logout();
        })
        .join();
    }
}
fn command(text: &str) -> Value {
    json!([{"tool":"run_command","arguments":{"command":text}}])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires account_demo built from the same integrated commit; run explicitly with --ignored"]
async fn encrypted_once_deny_and_two_phone_replay_observe_real_pty() -> Result<()> {
    let mut fixture = Fixture::start().await?;
    let phone = fixture.phone().await?;
    let other = fixture.phone().await?;
    phone
        .send(
            "once",
            command("/usr/bin/printf 'once\\n' >> auth-review-once.log"),
            Some(true),
        )
        .await?;
    let pending = phone.pending("approval").await?;
    ensure!(
        fixture.marker("auth-review-once.log").is_none(),
        "Legacy allow_input bypassed approval"
    );
    let requests = fixture.calls("once");
    tokio::time::sleep(Duration::from_millis(300)).await;
    ensure!(
        fixture.calls("once") == requests,
        "Human wait started extra model requests"
    );
    let (first, second) = tokio::join!(
        phone.resolve(&pending, "once-device-a", "once"),
        other.resolve(&pending, "once-device-b", "once")
    );
    ensure!(
        first.is_ok() || second.is_ok(),
        "Neither actual phone could resolve approval"
    );
    fixture
        .wait_marker("auth-review-once.log", "once\n")
        .await?;
    ensure!(
        phone.settled().await?["state"] == "completed",
        "Approved Run failed"
    );
    let (winner, request) = if first.is_ok() {
        (&phone, "once-device-a")
    } else {
        (&other, "once-device-b")
    };
    ensure!(
        winner.resolve(&pending, request, "once").await?["duplicate"] == true,
        "Resolve replay was not idempotent"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    ensure!(
        fixture.marker("auth-review-once.log").as_deref() == Some("once\n"),
        "Replay executed twice"
    );
    phone
        .send(
            "deny",
            command("/usr/bin/printf 'denied\\n' >> auth-review-denied.log"),
            None,
        )
        .await?;
    let denied = phone.pending("approval").await?;
    phone.resolve(&denied, "deny-one", "deny").await?;
    ensure!(
        phone.settled().await?["state"] == "completed",
        "Denial was not returned to model"
    );
    ensure!(
        fixture.marker("auth-review-denied.log").is_none(),
        "Denied command executed"
    );
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires account_demo built from the same integrated commit; run explicitly with --ignored"]
async fn encrypted_rule_regrant_remains_revocable_and_parameter_exact() -> Result<()> {
    let mut fixture = Fixture::start().await?;
    let phone = fixture.phone().await?;
    fs::write(fixture.dir.join("auth-review-source.log"), "first\n")?;
    let copy = "/bin/cp auth-review-source.log auth-review-rule.log";
    for iteration in 0..2 {
        let id = format!("always-{iteration}");
        phone.send(&id, command(copy), None).await?;
        let pending = phone.pending("approval").await?;
        ensure!(
            pending["can_always"] == true,
            "A fixed absolute program must support an exact permanent rule: {pending}"
        );
        ensure!(
            fixture.marker("auth-review-rule.log").is_none(),
            "Permanent command ran before approval"
        );
        phone
            .resolve(&pending, &format!("always-decision-{iteration}"), "always")
            .await?;
        fixture
            .wait_marker("auth-review-rule.log", "first\n")
            .await?;
        ensure!(
            phone.settled().await?["state"] == "completed",
            "Permanent command failed"
        );
        fs::remove_file(fixture.dir.join("auth-review-rule.log"))?;
        phone
            .send(&format!("rule-match-{iteration}"), command(copy), None)
            .await?;
        ensure!(
            phone.settled().await?["state"] == "completed",
            "Exact rule did not automatically execute"
        );
        fixture
            .wait_marker("auth-review-rule.log", "first\n")
            .await?;
        let rules = phone.rpc(json!({"action":"rules"})).await?;
        let rule = rules["items"]
            .as_array()
            .context("Rule list missing")?
            .first()
            .context("Rule absent")?;
        phone.rpc(json!({"action":"revoke_rule","request_id":format!("revoke-{iteration}"),"rule_id":rule["id"]})).await?;
        fs::remove_file(fixture.dir.join("auth-review-rule.log"))?;
    }
    phone
        .send("after-regrant-revoke", command(copy), None)
        .await?;
    phone.pending("approval").await?;
    ensure!(
        fixture.marker("auth-review-rule.log").is_none(),
        "Regranted rule remained active after revoke"
    );
    phone.rpc(json!({"action":"cancel"})).await?;
    phone.settled().await?;
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires account_demo built from the same integrated commit; run explicitly with --ignored"]
async fn encrypted_full_releases_approval_but_preserves_question_and_cancellation() -> Result<()> {
    let mut fixture = Fixture::start().await?;
    let phone = fixture.phone().await?;
    phone
        .send(
            "full-release",
            command("/usr/bin/printf 'full\\n' >> auth-review-full.log"),
            None,
        )
        .await?;
    phone.pending("approval").await?;
    phone.full(true).await?;
    fixture
        .wait_marker("auth-review-full.log", "full\n")
        .await?;
    ensure!(
        phone.settled().await?["state"] == "completed",
        "Full did not release pending approval"
    );
    phone.send("question",json!([{"tool":"ask_user","arguments":{"question":"Choose a fixture answer","options":["alpha","beta"]}}]),None).await?;
    let question = phone.pending("question").await?;
    let calls = fixture.calls("question");
    tokio::time::sleep(Duration::from_millis(300)).await;
    ensure!(
        fixture.calls("question") == calls,
        "Full automatically answered a human question"
    );
    phone.rpc(json!({"action":"resolve","request_id":"answer-beta","pending_id":question["id"],"answer":"beta"})).await?;
    ensure!(
        phone.settled().await?["state"] == "completed",
        "Answered question failed"
    );
    ensure!(
        fixture
            .results("question")?
            .iter()
            .any(|value| value["result"]["answer"] == "beta"),
        "The model did not receive the exact real-user answer"
    );
    phone.full(false).await?;
    phone
        .send(
            "cancel-pending",
            command("/usr/bin/printf 'bad\\n' >> auth-review-cancelled.log"),
            None,
        )
        .await?;
    let pending = phone.pending("approval").await?;
    phone.rpc(json!({"action":"cancel"})).await?;
    ensure!(
        phone.settled().await?["state"] == "cancelled",
        "Stop did not cancel the human wait"
    );
    ensure!(
        phone
            .resolve(&pending, "stale-after-cancel", "once")
            .await
            .is_err(),
        "Cancelled grant remained resolvable"
    );
    ensure!(
        fixture.marker("auth-review-cancelled.log").is_none(),
        "Cancelled command executed"
    );
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires account_demo built from the same integrated commit; run explicitly with --ignored"]
async fn encrypted_split_input_and_enter_are_separately_gated() -> Result<()> {
    let mut fixture = Fixture::start().await?;
    let phone = fixture.phone().await?;
    phone.send("split",json!([
        {"tool":"input_text","arguments":{"text":"/usr/bin/printf 'split\\n' >> auth-review-split.log","submit":false}},
        {"tool":"send_keys","arguments":{"key":"enter"}}
    ]),None).await?;
    let text = phone.pending("approval").await?;
    ensure!(
        text["tool"] == "input_text" && text["can_always"] == false,
        "Raw input was not conservatively gated"
    );
    phone.resolve(&text, "split-text", "once").await?;
    let enter = phone.pending("approval").await?;
    ensure!(
        enter["id"] != text["id"] && enter["tool"] == "send_keys",
        "Enter inherited the preceding once grant"
    );
    ensure!(
        fixture.marker("auth-review-split.log").is_none(),
        "Draft ran before Enter approval"
    );
    phone.resolve(&enter, "split-enter-deny", "deny").await?;
    ensure!(
        phone.settled().await?["state"] == "completed",
        "Denied Enter did not return to the model"
    );
    ensure!(
        fixture.marker("auth-review-split.log").is_none(),
        "Denied Enter executed the draft"
    );
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires account_demo built from the same integrated commit; run explicitly with --ignored"]
async fn encrypted_manual_preemption_blocks_a_previously_pending_action() -> Result<()> {
    let mut fixture = Fixture::start().await?;
    let phone = fixture.phone().await?;
    phone
        .send(
            "preempt",
            command("/usr/bin/printf 'stale\\n' >> auth-review-preempt.log"),
            None,
        )
        .await?;
    let pending = phone.pending("approval").await?;
    fixture.manual_input("/usr/bin/printf 'manual\\n' > auth-review-manual.log", true)?;
    fixture
        .wait_marker("auth-review-manual.log", "manual\n")
        .await?;
    // Resolution itself may already be invalidated, or execution may reject the
    // stale grant. In both cases the old PTY action must have no side effect.
    let _ = phone.resolve(&pending, "preempt-old-grant", "once").await;
    let state = phone.settled().await?;
    ensure!(
        matches!(
            state["state"].as_str(),
            Some("completed" | "paused" | "cancelled")
        ),
        "Unexpected preempted state: {state}"
    );
    ensure!(
        fixture.marker("auth-review-preempt.log").is_none(),
        "Approval bypassed manual input fence"
    );
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires account_demo built from the same integrated commit; run explicitly with --ignored"]
async fn encrypted_long_action_requires_exact_details_ack_and_preserves_target() -> Result<()> {
    let mut fixture = Fixture::start().await?;
    let phone = fixture.phone().await?;
    let payload = "ordinary".repeat(1400);
    let action = format!("/usr/bin/printf '%s\\n' '{payload}' > auth-review-details.log");
    phone.send("details", command(&action), None).await?;
    let pending = phone.pending("approval").await?;
    ensure!(
        pending["requires_details"] == true && pending["arguments_truncated"] == true,
        "Long action did not require details"
    );
    ensure!(
        fixture.marker("auth-review-details.log").is_none(),
        "Long action executed before review"
    );
    ensure!(phone.rpc(json!({"action":"resolve","request_id":"details-no-ack","pending_id":pending["id"],"decision":"once"})).await.is_err(), "Unacknowledged truncated action was approved");
    ensure!(phone.rpc(json!({"action":"resolve","request_id":"details-wrong-fingerprint","pending_id":pending["id"],"decision":"once","details_ack":true,"fingerprint":"v1:wrong"})).await.is_err(), "Wrong action fingerprint was approved");
    phone.resolve(&pending, "details-exact-ack", "once").await?;
    fixture
        .wait_marker("auth-review-details.log", &format!("{payload}\n"))
        .await?;
    ensure!(
        phone.settled().await?["state"] == "completed",
        "Complete details approval failed"
    );
    fixture.finish()?;
    Ok(())
}
