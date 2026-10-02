//! Explicit managed native execution. No Shell parsing and no PTY input mutation.
use super::*;
use ai_terminal_agent_runtime::store::Store;
use process_wrap::tokio::*;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    program: String,
    args: Vec<String>,
    stdin: Option<String>,
    session_id: Option<String>,
}
impl Arguments {
    fn parse(value: Value) -> Result<Self> {
        let args: Self =
            serde_json::from_value(value).context("invalid_native_program_arguments")?;
        ensure!(
            Path::new(&args.program).is_absolute()
                && args.program.len() <= 4096
                && !args.program.contains('\0'),
            "native_program_requires_absolute_path"
        );
        ensure!(
            args.args.len() <= 64
                && args.args.iter().map(String::len).sum::<usize>() <= 16000
                && args.args.iter().all(|s| !s.contains('\0')),
            "native_argument_limit"
        );
        ensure!(
            args.stdin.as_ref().is_none_or(|s| s.len() <= 16000),
            "native_stdin_limit"
        );
        Ok(args)
    }
}
pub(super) fn normalized_arguments(value: &Value) -> Result<Value> {
    let args = Arguments::parse(value.clone())?;
    let mut value = json!({"program":args.program,"args":args.args,"stdin":args.stdin});
    if let Some(session) = args.session_id {
        value["session_id"] = json!(session);
    }
    Ok(value)
}
struct Receipt {
    store: Arc<Store>,
    scope: Scope,
    id: String,
    settled: bool,
}
impl Drop for Receipt {
    fn drop(&mut self) {
        if !self.settled
            && let Ok(mut value) = self.store.command(&self.scope, &self.id)
        {
            value["state"] = json!("unknown");
            value["final"] = json!(true);
            value["reason"] = json!("native_execution_interrupted");
            value["exit_code"] = Value::Null;
            let _ = self.store.update_command(&self.scope, &self.id, &value);
        }
    }
}
fn stream(bytes: &[u8]) -> Value {
    if let Ok(text) = std::str::from_utf8(bytes) {
        json!({"encoding":"utf8","text":text})
    } else {
        json!({"encoding":"base64","data":STANDARD.encode(bytes)})
    }
}
impl Backend {
    pub(super) async fn run_program(
        &self,
        context: &ToolContext,
        value: Value,
    ) -> Result<ToolOutput> {
        let session = self.session(&value)?;
        let args = Arguments::parse(value)?;
        self.authorize(true).await?;
        context.check_authorization()?;
        let info = self.info(&session)?.info.context("session_unavailable")?;
        let cwd = crate::process::cwd(&info).context("current_session_cwd_unavailable")?;
        let store = self.host()?.agents.store.clone();
        let mut value = json!({"source":"native_program","program":args.program,"args":args.args,"cwd":cwd,"state":"prepared","accepted":false,"final":false,"exit_code":null,"epoch":info.epoch,"manual_revision":info.manual_revision,"evidence_source":"managed_native_process","stdin_bytes":args.stdin.as_ref().map_or(0,String::len),"submitted_at":now()});
        let command_id = store.begin_command(
            &self.scope,
            &context.run_id,
            &context.action_id,
            &session,
            value.clone(),
        )?;
        value["command_id"] = json!(command_id);
        value["session_id"] = json!(session);
        store.update_command(&self.scope, &command_id, &value)?;
        let mut receipt = Receipt {
            store: store.clone(),
            scope: self.scope.clone(),
            id: command_id.clone(),
            settled: false,
        };
        let mut command = CommandWrap::with_new(&args.program, |command| {
            command
                .args(&args.args)
                .current_dir(&cwd)
                .env_clear()
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .stdin(if args.stdin.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        #[cfg(unix)]
        command.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(JobObject);
        command.wrap(KillOnDrop);
        context.check_authorization()?;
        let started = Instant::now();
        let mut child = {
            let gate = context.execution_gate.lock().unwrap();
            ensure!(
                *gate && !*context.cancel.borrow(),
                "cancelled_before_native_spawn"
            );
            // This is the action-start linearization point. There are no redirects to
            // open before approval; each literal argv is passed directly to the kernel.
            context.commit_authorization(None)?;
            command.spawn().context("native_spawn_failed")?
        };
        value["state"] = json!("running");
        value["accepted"] = json!(true);
        store.update_command(&self.scope, &command_id, &value)?;
        let stdout = child.stdout().take().context("native_stdout_missing")?;
        let stderr = child.stderr().take().context("native_stderr_missing")?;
        let mut out = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        let mut err = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map(|_| bytes)
        });
        let input = if let Some(text) = args.stdin {
            let mut stdin = child.stdin().take().context("native_stdin_missing")?;
            Some(tokio::spawn(async move {
                stdin.write_all(text.as_bytes()).await?;
                stdin.shutdown().await
            }))
        } else {
            None
        };
        let mut cancel = context.cancel.clone();
        let limit = context.budget.remaining()?.min(Duration::from_secs(30));
        let result: Result<_> = tokio::select! {biased;
            _=cancel.wait_for(|v|*v)=>Err(anyhow::anyhow!("native_cancelled_outcome_unknown")),
            result=tokio::time::timeout(limit,async {let status=child.wait().await?;let stdout=(&mut out).await??;let stderr=(&mut err).await??;Ok::<_,anyhow::Error>((status,stdout,stderr))})=>result.context("native_timeout_outcome_unknown")?,
        };
        if let Some(input) = input {
            input.abort();
        }
        out.abort();
        err.abort();
        let (status, mut stdout, mut stderr) = result?;
        let stdout_truncated = stdout.len() > 65536;
        let stderr_truncated = stderr.len() > 65536;
        stdout.truncate(65536);
        stderr.truncate(65536);
        value["final"] = json!(true);
        value["state"] = json!(if status.code().is_some() {
            "completed"
        } else {
            "unknown"
        });
        value["exit_code"] = json!(status.code());
        value["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
        value["stdout_truncated"] = json!(stdout_truncated);
        value["stderr_truncated"] = json!(stderr_truncated);
        if status.code().is_none() {
            value["reason"] = json!("native_exit_code_unavailable");
        }
        store.update_command(&self.scope, &command_id, &value)?;
        receipt.settled = true;
        let body=json!({"source":"native_program","command_id":command_id,"stdout":stream(&stdout),"stderr":stream(&stderr),"exit_code":status.code(),"stdout_truncated":stdout_truncated,"stderr_truncated":stderr_truncated}).to_string();
        let preview = body
            .chars()
            .take(context.max_read_bytes / 4)
            .collect::<String>();
        Ok(ToolOutput {
            value: value.clone(),
            observation: Some(Observation {
                kind: "native_program".into(),
                metadata: value,
                body,
                model_body: Some(preview),
                binary: false,
                record_id: None,
            }),
            outcome: Some("written".into()),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_native_contract_enforces_utf8_byte_limits_and_defaults_to_eof() {
        assert!(normalized_arguments(&json!({"program":"relative","args":[]})).is_err());
        assert!(
            normalized_arguments(&json!({"program":"/bin/echo","args":["x".repeat(16001)]}))
                .is_err()
        );
        assert!(
            normalized_arguments(
                &json!({"program":"/bin/echo","args":[],"stdin":"中".repeat(6000)})
            )
            .is_err()
        );
        assert_eq!(
            normalized_arguments(&json!({"program":"/bin/echo","args":[]})).unwrap()["stdin"],
            Value::Null
        );
        assert_eq!(
            normalized_arguments(&json!({"program":"/bin/echo","args":["$HOME; literal"]}))
                .unwrap()["args"][0],
            "$HOME; literal"
        );
    }
}
