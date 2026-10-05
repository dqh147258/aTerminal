//! Safe process observations, separate from terminal parsing and authorization.
use std::path::PathBuf;

/// Retain a bounded prefix plus one truncation sentinel, but drain to EOF so
/// limiting evidence never closes the program's pipe and causes EPIPE/SIGPIPE.
pub(crate) async fn drain_output(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let retained = limit.saturating_add(1);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let length = reader.read(&mut buffer).await?;
        if length == 0 {
            return Ok(bytes);
        }
        let keep = length.min(retained.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
    }
}
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
fn output(program: &str, args: &[&str]) -> Option<String> {
    use std::{
        io::Read,
        time::{Duration, Instant},
    };
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        if start.elapsed() >= Duration::from_millis(750) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return None;
    }
    let bytes = rx.recv_timeout(Duration::from_millis(100)).ok()?.ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    String::from_utf8(bytes).ok()
}
pub(crate) fn identity(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        Some(format!(
            "proc:{}",
            stat.rsplit_once(") ")?.1.split_whitespace().nth(19)?
        ))
    }
    #[cfg(target_os = "macos")]
    {
        let value = output("/bin/ps", &["-p", &pid.to_string(), "-o", "lstart="])?;
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if value.is_empty() { None } else { Some(value) }
    }
    #[cfg(windows)]
    {
        let _ = pid;
        None
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        None
    }
}
pub(crate) fn cwd(info: &ai_terminal_protocol::local::SessionInfo) -> Option<PathBuf> {
    if info.exited || info.process_id == 0 || info.process_identity.is_empty() {
        return None;
    }
    if identity(info.process_id).as_deref() != Some(&info.process_identity) {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{}/cwd", info.process_id)).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let value = output(
            "/usr/sbin/lsof",
            &[
                "-a",
                "-p",
                &info.process_id.to_string(),
                "-d",
                "cwd",
                "-Fn",
                "-n",
                "-P",
            ],
        )?;
        value
            .lines()
            .find_map(|line| line.strip_prefix('n').map(PathBuf::from))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}
pub(crate) fn foreground(info: &ai_terminal_protocol::local::SessionInfo) -> serde_json::Value {
    #[cfg(unix)]
    {
        if info.foreground_group != 0
            && !info.exited
            && let Some(rows) = output("/bin/ps", &["-axo", "pid=,pgid=,lstart=,comm="])
        {
            let mut members = Vec::new();
            for row in rows.lines() {
                let columns = row.split_whitespace().collect::<Vec<_>>();
                if columns.len() < 8
                    || columns[1].parse::<u32>().ok() != Some(info.foreground_group)
                {
                    continue;
                }
                members.push(serde_json::json!({"pid":columns[0].parse::<u32>().ok(),"start_identity":columns[2..7].join(" "),"program":columns[7..].join(" ")}));
                if members.len() == 32 {
                    break;
                }
            }
            return serde_json::json!({"state":if members.is_empty(){"unknown"}else{"running"},"members":members,"process_group":info.foreground_group,"observed_at":chrono::Utc::now().timestamp_millis(),"evidence_source":"os_process_group"});
        }
    }
    serde_json::json!({"state":"unknown","members":[],"process_group":info.foreground_group,"evidence_source":"unavailable","not_application_completion":true})
}

/// OS corroboration for a host-maintained input boundary; shell text alone is insufficient.
/// The Actor must additionally prove that no input was written after the latest prompt.
pub(crate) fn shell_foreground(info: &ai_terminal_protocol::local::SessionInfo) -> bool {
    #[cfg(unix)]
    {
        !info.exited
            && info.process_id != 0
            && info.foreground_group == info.process_id
            && !info.process_identity.is_empty()
            && identity(info.process_id).as_deref() == Some(info.process_identity.as_str())
    }
    #[cfg(not(unix))]
    {
        let _ = info;
        false
    }
}

/// A bounded native read selected by the policy's fixed-program plan, never Shell source.
pub(crate) struct ReadCommandResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u64,
}
pub(crate) async fn inspect(
    context: &ai_terminal_agent_runtime::host::ToolContext,
    program: &std::path::Path,
    argv: &[String],
    cwd: &std::path::Path,
) -> anyhow::Result<ReadCommandResult> {
    use anyhow::{Context as _, ensure};
    use process_wrap::tokio::*;
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    use tokio::io::AsyncReadExt;
    ensure!(
        program.is_absolute() && cwd.is_absolute(),
        "inspect_requires_absolute_program_and_cwd"
    );
    ensure!(
        argv.len() <= 128 && argv.iter().map(String::len).sum::<usize>() <= 16000,
        "inspect_argument_limit"
    );
    // Clear program-specific startup injection, loaders, pagers and locale-dependent parsing.
    let mut command = CommandWrap::with_new(program, |command| {
        command
            .args(argv)
            .current_dir(cwd)
            .env_clear()
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command.wrap(KillOnDrop);
    // The Broker supplies an observation permit that rechecks identity/cwd at
    // commit. Reentering check_authorization while this gate is held would lock
    // the same non-reentrant mutex; commit does not acquire the gate again.
    let start = Instant::now();
    let mut child = {
        let gate = context.execution_gate.lock().unwrap();
        ensure!(
            *gate && !*context.cancel.borrow(),
            "cancelled_before_inspect_spawn"
        );
        context.commit_authorization(None)?;
        command.spawn()?
    };
    let stdout = child.stdout().take().context("inspect_stdout_missing")?;
    let stderr = child.stderr().take().context("inspect_stderr_missing")?;
    let out_limit = context.max_read_bytes.clamp(4, 65536);
    let err_limit = 8192usize;
    let mut out = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take(out_limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let mut err = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take(err_limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let mut cancelled = context.cancel.clone();
    let result = {
        let work = async {
            let status = child.wait().await?;
            let stdout = (&mut out).await??;
            let stderr = (&mut err).await??;
            Ok::<_, anyhow::Error>((status, stdout, stderr))
        };
        tokio::pin!(work);
        loop {
            let remaining = match context.budget.remaining() {
                Ok(value) => value,
                Err(error) => break Err(error),
            };
            if *cancelled.borrow() {
                break Err(anyhow::anyhow!("cancelled"));
            }
            if start.elapsed() >= Duration::from_secs(30) {
                break Err(anyhow::anyhow!("inspect_timeout"));
            }
            tokio::select! {biased;
                _=cancelled.wait_for(|value|*value)=>break Err(anyhow::anyhow!("cancelled")),
                result=&mut work=>break result,
                _=tokio::time::sleep(remaining.min(Duration::from_millis(100)))=>{}
            }
        }
    };
    // End borrows before kill/abort. Dropping a cancelled runner also kills its process group.
    if result.is_err() {
        let _ = child.start_kill();
        out.abort();
        err.abort();
    }
    let (status, mut stdout, mut stderr) = result?;
    let stdout_truncated = stdout.len() > out_limit;
    let stderr_truncated = stderr.len() > err_limit;
    stdout.truncate(out_limit);
    stderr.truncate(err_limit);
    context.budget.remaining()?;
    ensure!(!*cancelled.borrow(), "cancelled");
    context.budget.read(stdout.len() + stderr.len())?;
    Ok(ReadCommandResult {
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        exit_code: status.code(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

#[cfg(all(test, unix))]
mod toolset_tests {
    use super::*;
    use ai_terminal_agent_runtime::{
        host::{Budget, ToolContext},
        store::Store,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    pub(super) fn context(
        temp: &tempfile::TempDir,
    ) -> (ToolContext, tokio::sync::watch::Sender<bool>) {
        let store = Store::open(&temp.path().join("state/db")).unwrap();
        let scope = store.agent("o", "d", Some("s")).unwrap();
        let root = store
            .accept_user(&scope, "r", "inspect", json!({}))
            .unwrap();
        let (cancel, receiver) = tokio::sync::watch::channel(false);
        (
            ToolContext {
                history_unit_id: root.user_message_id,
                vision: false,
                scope: scope.clone(),
                run_id: root.run_id,
                root_user_message_id: root.root_user_message_id,
                action_id: "inspect".into(),
                max_read_bytes: 1024,
                budget: Arc::new(Budget::new(30, 10, 10000, scope)),
                cancel: receiver,
                execution_gate: Arc::new(Mutex::new(true)),
                authorization_check: None,
            },
            cancel,
        )
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn native_inspection_uses_observed_directory_and_bounded_streams() {
        let temp = tempfile::tempdir().unwrap();
        let (context, _cancel) = context(&temp);
        let result = inspect(&context, std::path::Path::new("/bin/pwd"), &[], temp.path())
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(
            String::from_utf8(result.stdout).unwrap().trim(),
            temp.path().canonicalize().unwrap().to_string_lossy()
        );
        let path = temp.path().join("large");
        std::fs::write(&path, vec![b'x'; 128 * 1024]).unwrap();
        let result = inspect(
            &context,
            std::path::Path::new("/bin/cat"),
            &[path.to_string_lossy().into_owned()],
            temp.path(),
        )
        .await
        .unwrap();
        assert!(result.stdout_truncated);
        assert_eq!(result.stdout.len(), 1024);
        assert!(result.stderr.len() <= 8192);
        let missing = inspect(
            &context,
            std::path::Path::new("/bin/cat"),
            &["missing".into()],
            temp.path(),
        )
        .await
        .unwrap();
        assert_ne!(missing.exit_code, Some(0));
        assert!(!missing.stderr.is_empty());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn native_inspection_cancellation_kills_without_waiting_for_process_exit() {
        let temp = tempfile::tempdir().unwrap();
        let (context, cancel) = context(&temp);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            cancel.send(true).unwrap();
        });
        let start = std::time::Instant::now();
        assert_eq!(
            inspect(
                &context,
                std::path::Path::new("/bin/sleep"),
                &["30".into()],
                temp.path()
            )
            .await
            .err()
            .unwrap()
            .to_string(),
            "cancelled"
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
}

#[cfg(all(test, unix))]
mod toolset_permit_tests {
    use super::*;
    use ai_terminal_agent_runtime::host::AuthorizationPermit;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };
    #[tokio::test]
    async fn inspection_commits_observation_once_and_revalidation_can_prevent_spawn() {
        let temp = tempfile::tempdir().unwrap();
        let (mut context, _cancel) = super::toolset_tests::context(&temp);
        let checks = Arc::new(AtomicU32::new(0));
        let verified = checks.clone();
        context.authorization_check = Some(AuthorizationPermit::observation(move || {
            verified.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }));
        let result = inspect(&context, std::path::Path::new("/bin/pwd"), &[], temp.path())
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(checks.load(Ordering::Acquire), 1);
        assert_eq!(
            inspect(&context, std::path::Path::new("/bin/pwd"), &[], temp.path())
                .await
                .err()
                .unwrap()
                .to_string(),
            "authorization_action_already_consumed"
        );
        context.authorization_check = Some(AuthorizationPermit::observation(|| {
            anyhow::bail!("inspect_fixture_cwd_changed")
        }));
        assert_eq!(
            inspect(&context, std::path::Path::new("/bin/pwd"), &[], temp.path())
                .await
                .err()
                .unwrap()
                .to_string(),
            "inspect_fixture_cwd_changed"
        );
    }
}
