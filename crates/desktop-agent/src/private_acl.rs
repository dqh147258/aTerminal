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
    let script = r#"function Write-ATerminalAclTiming([string]$stage) { if($env:ATERMINAL_ACL_TRACE -eq '1') { [Console]::Error.WriteLine('Agent ACL timing: '+$stage+' utc='+[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ',[Globalization.CultureInfo]::InvariantCulture)) } }; Write-ATerminalAclTiming 'entry'; $ErrorActionPreference='Stop'; $p=$env:ATERMINAL_ACL_PATH; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; if($env:ATERMINAL_ACL_CREATE -eq '1'){ if($env:ATERMINAL_ACL_DIRECTORY -eq '1'){$acl=New-Object Security.AccessControl.DirectorySecurity; $flags=[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'}else{$acl=New-Object Security.AccessControl.FileSecurity; $flags=[Security.AccessControl.InheritanceFlags]::None}; $acl.SetOwner($sid); $acl.SetAccessRuleProtection($true,$false); $rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl',$flags,'None','Allow'); $acl.AddAccessRule($rule); Write-ATerminalAclTiming 'set-start'; Set-Acl -LiteralPath $p -AclObject $acl; Write-ATerminalAclTiming 'set-ready' }; Write-ATerminalAclTiming 'get-start'; $a=Get-Acl -LiteralPath $p; Write-ATerminalAclTiming 'get-ready'; if($a.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value){throw 'owner mismatch'}; foreach($r in $a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($sid.Value,'S-1-5-18','S-1-5-32-544')){throw 'non-private ACL'}}; Write-ATerminalAclTiming 'exit'"#;
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
