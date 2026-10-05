//! Windows ACL handling uses fixed PowerShell code and a literal path passed via
//! the environment. No path or account text is interpolated into executable code.
#[cfg(windows)]
pub(crate) fn protect(
    path: &std::path::Path,
    created: bool,
    directory: bool,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let system = std::env::var_os("SystemRoot").context("Windows SystemRoot unavailable")?;
    let program =
        std::path::Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = r#"$ErrorActionPreference='Stop'; $p=$env:ATERMINAL_ACL_PATH; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; if($env:ATERMINAL_ACL_CREATE -eq '1'){ if($env:ATERMINAL_ACL_DIRECTORY -eq '1'){$acl=New-Object Security.AccessControl.DirectorySecurity; $flags=[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'}else{$acl=New-Object Security.AccessControl.FileSecurity; $flags=[Security.AccessControl.InheritanceFlags]::None}; $acl.SetOwner($sid); $acl.SetAccessRuleProtection($true,$false); $rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl',$flags,'None','Allow'); $acl.AddAccessRule($rule); Set-Acl -LiteralPath $p -AclObject $acl }; $a=Get-Acl -LiteralPath $p; if($a.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value){throw 'owner mismatch'}; foreach($r in $a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($sid.Value,'S-1-5-18','S-1-5-32-544')){throw 'non-private ACL'}}"#;
    crate::service::startup_stage("acl:powershell-start");
    let output = std::process::Command::new(program)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env_clear()
        .env("SystemRoot", system)
        .env("ATERMINAL_ACL_PATH", path)
        .env("ATERMINAL_ACL_CREATE", if created { "1" } else { "0" })
        .env("ATERMINAL_ACL_DIRECTORY", if directory { "1" } else { "0" })
        .output()?;
    crate::service::startup_stage("acl:powershell-ready");
    ensure!(
        output.status.success(),
        "state path must have a private ACL owned by the current Windows user"
    );
    Ok(())
}
