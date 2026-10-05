//! Windows ACL handling uses fixed PowerShell code and a literal path passed via
//! the environment. No path or account text is interpolated into executable code.
#[cfg(windows)]
pub(crate) fn protect(
    path: &std::path::Path,
    created: bool,
    directory: bool,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").context("Windows SystemRoot unavailable")?;
    let program =
        std::path::Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let trace = cfg!(debug_assertions) && std::env::var_os("ATERMINAL_STARTUP_TRACE").is_some();
    // Direct .NET calls avoid PowerShell cmdlet/module discovery under env_clear.
    // Owner, protected DACL, and fresh explicit/inherited ACE validation are unchanged.
    let script = r#"function Write-ATerminalAclTiming([string]$stage) { if($env:ATERMINAL_ACL_TRACE -eq '1') { [Console]::Error.WriteLine('Agent ACL timing: '+$stage+' utc='+[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ',[Globalization.CultureInfo]::InvariantCulture)) } }; Write-ATerminalAclTiming 'entry'; $ErrorActionPreference='Stop'; $p=$env:ATERMINAL_ACL_PATH; $directory=($env:ATERMINAL_ACL_DIRECTORY -eq '1'); $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; if($env:ATERMINAL_ACL_CREATE -eq '1'){ if($directory){$acl=[Security.AccessControl.DirectorySecurity]::new(); $flags=[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'}else{$acl=[Security.AccessControl.FileSecurity]::new(); $flags=[Security.AccessControl.InheritanceFlags]::None}; $acl.SetOwner($sid); $acl.SetAccessRuleProtection($true,$false); $rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,[Security.AccessControl.FileSystemRights]::FullControl,$flags,[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow); $acl.AddAccessRule($rule); Write-ATerminalAclTiming 'set-start'; if($directory){[IO.Directory]::SetAccessControl($p,$acl)}else{[IO.File]::SetAccessControl($p,$acl)}; Write-ATerminalAclTiming 'set-ready' }; Write-ATerminalAclTiming 'get-start'; $a=if($directory){[IO.Directory]::GetAccessControl($p)}else{[IO.File]::GetAccessControl($p)}; Write-ATerminalAclTiming 'get-ready'; if($a.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value){throw 'owner mismatch'}; foreach($r in $a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($sid.Value,'S-1-5-18','S-1-5-32-544')){throw 'non-private ACL'}}; Write-ATerminalAclTiming 'exit'"#;
    crate::service::startup_stage("acl:powershell-start");
    let output = std::process::Command::new(program)
        // The ACL helper has no interactive console and must not attach to one.
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env_clear()
        .env("SystemRoot", system)
        .env("ATERMINAL_ACL_TRACE", if trace { "1" } else { "0" })
        .env("ATERMINAL_ACL_PATH", path)
        .env("ATERMINAL_ACL_CREATE", if created { "1" } else { "0" })
        .env("ATERMINAL_ACL_DIRECTORY", if directory { "1" } else { "0" })
        .output()?;
    if trace {
        for line in String::from_utf8_lossy(&output.stderr).lines() {
            if let Some((stage, utc)) = timing_marker(line) {
                eprintln!("Agent ACL timing: {stage} utc={utc}");
            }
        }
    }
    crate::service::startup_stage("acl:powershell-ready");
    ensure!(
        output.status.success(),
        "state path must have a private ACL owned by the current Windows user"
    );
    Ok(())
}

// Accept only fixed marker names and a complete UTC millisecond timestamp.
#[cfg(any(windows, test))]
fn timing_marker(line: &str) -> Option<(&str, &str)> {
    let (stage, utc) = line
        .strip_prefix("Agent ACL timing: ")?
        .split_once(" utc=")?;
    if !matches!(
        stage,
        "entry" | "set-start" | "set-ready" | "get-start" | "get-ready" | "exit"
    ) {
        return None;
    }
    let bytes = utc.as_bytes();
    if bytes.len() != 24
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    Some((stage, utc))
}

#[cfg(test)]
mod tests {
    use super::timing_marker;

    #[cfg(windows)]
    fn assert_persisted_private_acl(path: &std::path::Path, directory: bool) {
        use std::os::windows::process::CommandExt;
        let system = std::env::var_os("SystemRoot").unwrap();
        let program =
            std::path::Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let inspection_script = r#"$ErrorActionPreference='Stop'; $p=$env:ATERMINAL_ACL_PATH; $directory=($env:ATERMINAL_ACL_DIRECTORY -eq '1'); $a=if($directory){[IO.Directory]::GetAccessControl($p)}else{[IO.File]::GetAccessControl($p)}; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; if($a.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value -or -not $a.AreAccessRulesProtected){throw 'owner or protection mismatch'}; $flags=[Security.AccessControl.InheritanceFlags]::None; if($directory){$flags=[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'}; $rules=$a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]); if($rules.Count -ne 1){throw 'unexpected rule count'}; $r=$rules[0]; if($r.IdentityReference.Value -ne $sid.Value -or $r.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or $r.FileSystemRights -ne [Security.AccessControl.FileSystemRights]::FullControl -or $r.InheritanceFlags -ne $flags -or $r.PropagationFlags -ne [Security.AccessControl.PropagationFlags]::None -or $r.IsInherited){throw 'unexpected owner rule'}"#;
        let output = std::process::Command::new(program)
            .creation_flags(0x08000000)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                inspection_script,
            ])
            .env_clear()
            .env("SystemRoot", system)
            .env("ATERMINAL_ACL_PATH", path)
            .env("ATERMINAL_ACL_DIRECTORY", if directory { "1" } else { "0" })
            .output()
            .unwrap();
        assert!(output.status.success(), "persisted private ACL differs");
    }

    #[cfg(windows)]
    fn allow_everyone(path: &std::path::Path, directory: bool, inherit: bool) {
        use std::os::windows::process::CommandExt;
        let system = std::env::var_os("SystemRoot").unwrap();
        let program =
            std::path::Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        // Test-owned empty objects only. Use the same literal-path boundary,
        // and direct .NET calls so the fixture cannot hide an autoload delay.
        let mutation_script = r#"$ErrorActionPreference='Stop'; $p=$env:ATERMINAL_ACL_PATH; $directory=($env:ATERMINAL_ACL_DIRECTORY -eq '1'); $a=if($directory){[IO.Directory]::GetAccessControl($p)}else{[IO.File]::GetAccessControl($p)}; $flags=[Security.AccessControl.InheritanceFlags]::None; if($directory -and $env:ATERMINAL_ACL_INHERIT -eq '1'){$flags=[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'}; $everyone=[Security.Principal.SecurityIdentifier]::new('S-1-1-0'); $rule=[Security.AccessControl.FileSystemAccessRule]::new($everyone,[Security.AccessControl.FileSystemRights]::ReadAndExecute,$flags,[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow); $a.AddAccessRule($rule); if($directory){[IO.Directory]::SetAccessControl($p,$a)}else{[IO.File]::SetAccessControl($p,$a)}"#;
        let output = std::process::Command::new(program)
            .creation_flags(0x08000000)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                mutation_script,
            ])
            .env_clear()
            .env("SystemRoot", system)
            .env("ATERMINAL_ACL_PATH", path)
            .env("ATERMINAL_ACL_DIRECTORY", if directory { "1" } else { "0" })
            .env("ATERMINAL_ACL_INHERIT", if inherit { "1" } else { "0" })
            .output()
            .unwrap();
        assert!(output.status.success(), "ACL mutation fixture failed");
    }

    #[cfg(windows)]
    #[test]
    fn creates_and_revalidates_private_literal_directory_and_file() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("literal [directory]; $value");
        std::fs::create_dir(&directory).unwrap();
        super::protect(&directory, true, true).unwrap();
        assert_persisted_private_acl(&directory, true);
        super::protect(&directory, false, true).unwrap();
        let file = directory.join("literal [file]; $value.json");
        std::fs::write(&file, b"{}").unwrap();
        super::protect(&file, true, false).unwrap();
        assert_persisted_private_acl(&file, false);
        super::protect(&file, false, false).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn missing_paths_fail_closed_without_creation() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        assert!(super::protect(&missing, false, true).is_err());
        assert!(super::protect(&missing, false, false).is_err());
        assert!(!missing.exists());
    }

    #[cfg(windows)]
    #[test]
    fn changed_explicit_allow_entries_are_rejected_without_repair() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        std::fs::create_dir(&directory).unwrap();
        super::protect(&directory, true, true).unwrap();
        let file = directory.join("state.json");
        std::fs::write(&file, b"{}").unwrap();
        super::protect(&file, true, false).unwrap();
        allow_everyone(&file, false, false);
        assert!(super::protect(&file, false, false).is_err());
        assert!(super::protect(&file, false, false).is_err());
        allow_everyone(&directory, true, false);
        assert!(super::protect(&directory, false, true).is_err());
        assert!(super::protect(&directory, false, true).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn inherited_allow_entries_are_rejected_until_new_objects_are_protected() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        super::protect(&parent, true, true).unwrap();
        allow_everyone(&parent, true, true);
        let directory = parent.join("child");
        std::fs::create_dir(&directory).unwrap();
        let file = parent.join("child.json");
        std::fs::write(&file, b"{}").unwrap();
        assert!(super::protect(&directory, false, true).is_err());
        assert!(super::protect(&file, false, false).is_err());
        super::protect(&directory, true, true).unwrap();
        super::protect(&file, true, false).unwrap();
        assert_persisted_private_acl(&directory, true);
        assert_persisted_private_acl(&file, false);
        super::protect(&directory, false, true).unwrap();
        super::protect(&file, false, false).unwrap();
    }

    #[test]
    fn timing_output_only_allows_fixed_stages_and_utc_timestamps() {
        let utc = "2026-10-05T05:54:00.123Z";
        for stage in [
            "entry",
            "set-start",
            "set-ready",
            "get-start",
            "get-ready",
            "exit",
        ] {
            let line = format!("Agent ACL timing: {stage} utc={utc}");
            assert_eq!(timing_marker(&line), Some((stage, utc)));
        }
        for line in [
            "unrelated stderr",
            "Agent ACL timing: owner utc=2026-10-05T05:54:00.123Z",
            "Agent ACL timing: entry utc=private data",
            "Agent ACL timing: entry utc=2026-10-05T05:54:00.123Z trailing data",
            "Agent ACL timing: entry utc=2026-10-05T05:54:00.123+00:00",
        ] {
            assert!(timing_marker(line).is_none());
        }
    }
}
