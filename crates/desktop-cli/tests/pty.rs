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
    let attempts = if cfg!(windows) { 16 } else { 1 };
    for piped in [false, true] {
        for attempt in 0..attempts {
            let file = std::env::temp_dir().join(format!(
                "ai-terminal-exit-{}-{piped}-{attempt}.pb",
                std::process::id()
            ));
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_aTerminal"));
            cmd.arg("--snapshot").arg(&file).arg("--");
            #[cfg(unix)]
            cmd.args(["/bin/sh", "-c", "exit 7"]);
            #[cfg(windows)]
            cmd.args(["powershell.exe", "-NoProfile", "-Command", "exit 7"]);
            // Exercise both the original inherited-console path and redirected capture.
            let (status, stderr) = if piped {
                let output = cmd.output().expect("start piped capture");
                (
                    output.status,
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            } else {
                (
                    cmd.status().expect("start inherited capture"),
                    String::new(),
                )
            };
            // Inspect the snapshot before asserting the status so failure distinguishes
            // a completed capture from wrapper termination before it writes the file.
            let snapshot = std::fs::read(&file)
                .map_err(|error| error.to_string())
                .and_then(|bytes| Snapshot::from_wire(&bytes).map_err(|error| error.to_string()))
                .map(|_| ());
            let _ = std::fs::remove_file(&file);
            assert_eq!(
                status.code(),
                Some(7),
                "piped={piped}, attempt={attempt}, snapshot={snapshot:?}, stderr={stderr}"
            );
            assert!(
                snapshot.is_ok(),
                "piped={piped}, attempt={attempt}: {snapshot:?}"
            );
        }
    }
}

#[test]
fn large_output_is_drained_before_reporting_child_exit() {
    let file = std::env::temp_dir().join(format!("ai-terminal-tail-{}.pb", std::process::id()));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aTerminal"));
    cmd.args(["--rows", "8", "--cols", "40", "--snapshot"])
        .arg(&file)
        .arg("--");
    #[cfg(unix)]
    cmd.args([
        "/bin/sh",
        "-c",
        "i=0; while [ \"$i\" -lt 4096 ]; do printf '%064d' 0; i=$((i + 1)); done; printf '\\r\\nFINAL_OUTPUT'; exit 19",
    ]);
    #[cfg(windows)]
    cmd.args([
        "powershell.exe",
        "-NoProfile",
        "-Command",
        "[Console]::Write(('x' * 262144)); [Console]::Write(\"`r`nFINAL_OUTPUT\"); exit 19",
    ]);
    let output = cmd.output().expect("start terminal capture");
    assert_eq!(
        output.status.code(),
        Some(19),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = Snapshot::from_wire(&std::fs::read(&file).unwrap()).unwrap();
    let text = snapshot
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("FINAL_OUTPUT"),
        "missing final output: {text:?}"
    );
    std::fs::remove_file(file).unwrap();
}
