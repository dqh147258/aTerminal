//! Opt-in, session-local hooks. Hook reports are observations, never authorization.
use anyhow::{Result, ensure};
use portable_pty::CommandBuilder;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
};
pub(crate) struct Integration {
    directory: PathBuf,
}
impl Integration {
    pub fn prepare(root: &Path, command: &[OsString]) -> Result<(Self, CommandBuilder)> {
        let executable = command
            .first()
            .cloned()
            .unwrap_or_else(super::pty::default_shell);
        let name = Path::new(&executable)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        ensure!(
            command.iter().skip(1).all(|s| s == "-i"),
            "shell_integration_requires_interactive_shell_without_command_flags"
        );
        ensure!(
            ["bash", "zsh", "pwsh", "powershell"].contains(&name.as_str()),
            "shell_integration_unsupported"
        );
        let directory = root
            .join("runtime/shell")
            .join(format!("{:016x}", crate::random_id()));
        crate::service::secure_dir(&directory)?;
        let event = directory.join("state");
        let mut cmd = CommandBuilder::new(executable);
        cmd.env("ATERMINAL_SHELL_STATE", event.as_os_str());
        match name.as_str() {
            "bash" => {
                let path = directory.join("bashrc");
                write(
                    &path,
                    r#"[[ -f "$HOME/.bashrc" ]] && source "$HOME/.bashrc"
__aterminal_report() { local code=$?; printf '%s\0%s\0%s\0' "$1" "$code" "$PWD" > "$ATERMINAL_SHELL_STATE"; return "$code"; }
__aterminal_prompt() { __aterminal_report prompt; }
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == 'declare -a'* ]]; then
  PROMPT_COMMAND=(__aterminal_prompt "${PROMPT_COMMAND[@]}")
else
  PROMPT_COMMAND="__aterminal_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
fi
# Preserve an existing DEBUG trap. Foreground process observations remain available.
if [[ -z $(trap -p DEBUG) ]]; then
  trap 'case "$BASH_COMMAND" in __aterminal_*|"$PROMPT_COMMAND") ;; *) __aterminal_report running ;; esac' DEBUG
fi
"#,
                )?;
                cmd.arg("--rcfile");
                cmd.arg(path);
                cmd.arg("-i");
            }
            "zsh" => {
                cmd.env(
                    "ATERMINAL_ORIGINAL_ZDOTDIR",
                    std::env::var_os("ZDOTDIR")
                        .unwrap_or_else(|| std::env::var_os("HOME").unwrap_or_default()),
                );
                cmd.env("ZDOTDIR", directory.as_os_str());
                write(
                    &directory.join(".zshenv"),
                    r#"[[ -f "$ATERMINAL_ORIGINAL_ZDOTDIR/.zshenv" ]] && source "$ATERMINAL_ORIGINAL_ZDOTDIR/.zshenv"
"#,
                )?;
                write(
                    &directory.join(".zshrc"),
                    r#"ZDOTDIR="$ATERMINAL_ORIGINAL_ZDOTDIR"
[[ -f "$ZDOTDIR/.zshrc" ]] && source "$ZDOTDIR/.zshrc"
__aterminal_precmd() { local code=$?; printf '%s\0%s\0%s\0' prompt "$code" "$PWD" > "$ATERMINAL_SHELL_STATE"; return "$code"; }
__aterminal_preexec() { printf '%s\0%s\0%s\0' running unknown "$PWD" > "$ATERMINAL_SHELL_STATE"; }
autoload -Uz add-zsh-hook
add-zsh-hook precmd __aterminal_precmd
add-zsh-hook preexec __aterminal_preexec
"#,
                )?;
                cmd.arg("-i");
            }
            _ => {
                let path = directory.join("init.ps1");
                write(
                    &path,
                    r#"$global:ATerminalOriginalPrompt = $function:prompt
function global:prompt {
  $ok = $?; $code = $global:LASTEXITCODE
  $value = 'prompt' + [char]0 + $(if ($null -ne $code) { [string]$code } elseif ($ok) { '0' } else { '1' }) + [char]0 + $PWD.Path + [char]0
  [System.IO.File]::WriteAllText($env:ATERMINAL_SHELL_STATE, $value, [System.Text.UTF8Encoding]::new($false))
  if ($global:ATerminalOriginalPrompt) { & $global:ATerminalOriginalPrompt } else { 'PS ' + $PWD.Path + '> ' }
}
"#,
                )?;
                cmd.arg("-NoExit");
                cmd.arg("-File");
                cmd.arg(path);
            }
        }
        Ok((Self { directory }, cmd))
    }
    pub fn observation(&self) -> Option<Value> {
        let reported_at = std::fs::metadata(self.directory.join("state"))
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis();
        let mut bytes = Vec::new();
        std::fs::File::open(self.directory.join("state"))
            .ok()?
            .take(16385)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 16384 || bytes.last() != Some(&0) {
            return None;
        }
        let parts = bytes.split(|b| *b == 0).collect::<Vec<_>>();
        if parts.len() != 4 {
            return None;
        }
        let phase = std::str::from_utf8(parts[0]).ok()?;
        if !["prompt", "running"].contains(&phase) {
            return None;
        }
        Some(
            json!({"phase":phase,"cwd":std::str::from_utf8(parts[2]).ok()?,"exit_code":if phase=="prompt"{std::str::from_utf8(parts[1]).ok()?.parse::<i32>().ok()}else{None},"evidence_source":"session_shell_hook","reported_at":reported_at,"trusted_for_authorization":false}),
        )
    }
}
fn write(path: &Path, body: &str) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
impl Drop for Integration {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
