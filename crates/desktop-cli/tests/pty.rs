use ai_terminal_protocol::Snapshot;
use std::process::Command;

#[test]
fn actual_pty_output_survives_the_engine_and_binary_protocol() {
    let file = std::env::temp_dir().join(format!("ai-terminal-pty-{}.pb", std::process::id()));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aTerminal"));
    cmd.args([
        "--rows",
        "8",
        "--cols",
        "40",
        "--timeout-secs",
        "10",
        "--snapshot",
    ])
    .arg(&file)
    .arg("--");
    #[cfg(unix)]
    cmd.args([
        "/bin/sh",
        "-c",
        "printf 'hello \\033[31mRED\\033[0m\\r\\nPTY_OK'",
    ]);
    #[cfg(windows)]
    cmd.args([
        "powershell.exe",
        "-NoProfile",
        "-Command",
        "Write-Output 'hello RED'; Write-Output 'PTY_OK'",
    ]);
    let output = cmd.output().expect("start terminal capture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = Snapshot::from_wire(&std::fs::read(&file).unwrap()).unwrap();
    let text = snapshot
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>();
    assert!(text.contains("hello RED"));
    assert!(text.contains("PTY_OK"));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn child_failure_is_not_reported_as_success() {
    let file = std::env::temp_dir().join(format!("ai-terminal-exit-{}.pb", std::process::id()));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aTerminal"));
    cmd.arg("--snapshot").arg(&file).arg("--");
    #[cfg(unix)]
    cmd.args(["/bin/sh", "-c", "exit 7"]);
    #[cfg(windows)]
    cmd.args(["powershell.exe", "-NoProfile", "-Command", "exit 7"]);
    assert_eq!(cmd.status().unwrap().code(), Some(7));
    std::fs::remove_file(file).unwrap();
}
