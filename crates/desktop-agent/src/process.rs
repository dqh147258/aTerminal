//! Safe process observations, separate from terminal parsing and authorization.
use std::path::PathBuf;
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
        return Some(format!(
            "proc:{}",
            stat.rsplit_once(") ")?.1.split_whitespace().nth(19)?
        ));
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
