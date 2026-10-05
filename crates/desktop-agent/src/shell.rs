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
__aterminal_association=0; __aterminal_seq=0; __aterminal_ready=0; __aterminal_command=; __aterminal_previous_history=
__aterminal_emit() { printf '%s\0%s\0%s\0%s\0%s\0%s\0' "$1" "$2" "$PWD" "$__aterminal_seq" "$__aterminal_command" "bash:$__aterminal_association" > "$ATERMINAL_SHELL_STATE.tmp"; command mv -f -- "$ATERMINAL_SHELL_STATE.tmp" "$ATERMINAL_SHELL_STATE"; }
__aterminal_capture() { __aterminal_code=$?; __aterminal_ready=0; return "$__aterminal_code"; }
__aterminal_arm() { __aterminal_emit prompt "$__aterminal_code"; __aterminal_previous_history=$(HISTTIMEFORMAT= builtin history 1); __aterminal_ready=1; return "$__aterminal_code"; }
__aterminal_before() {
  [[ $__aterminal_ready == 1 && $BASH_SUBSHELL == 0 ]] || return
  case "$BASH_COMMAND" in __aterminal_*) return ;; esac
  __aterminal_ready=0; __aterminal_seq=$((__aterminal_seq + 1)); __aterminal_command=
  local item; item=$(HISTTIMEFORMAT= builtin history 1)
  if [[ "$item" != "$__aterminal_previous_history" && "$item" =~ ^[[:space:]]*[0-9]+[[:space:]]+(.+)$ ]]; then __aterminal_command=${BASH_REMATCH[1]}; fi
  __aterminal_emit running unknown
}
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == 'declare -a'* ]]; then
  PROMPT_COMMAND=(__aterminal_capture "${PROMPT_COMMAND[@]}" __aterminal_arm)
else
  PROMPT_COMMAND="__aterminal_capture${PROMPT_COMMAND:+; $PROMPT_COMMAND}; __aterminal_arm"
fi
# Never replace a user's DEBUG trap: missing preexec evidence remains unknown.
if [[ -z $(trap -p DEBUG) ]]; then __aterminal_association=1; trap '__aterminal_before' DEBUG; fi
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
__aterminal_seq=0; __aterminal_command=
__aterminal_emit() { printf '%s\0%s\0%s\0%s\0%s\0%s\0' "$1" "$2" "$PWD" "$__aterminal_seq" "$__aterminal_command" zsh:1 > "$ATERMINAL_SHELL_STATE.tmp"; command mv -f -- "$ATERMINAL_SHELL_STATE.tmp" "$ATERMINAL_SHELL_STATE"; }
__aterminal_precmd() { local code=$?; __aterminal_emit prompt "$code"; return "$code"; }
__aterminal_preexec() { __aterminal_seq=$((__aterminal_seq + 1)); __aterminal_command=$1; __aterminal_emit running unknown; }
autoload -Uz add-zsh-hook
# Capture the command's status before other precmd hooks can replace it.
if (( $+functions[precmd] )); then
  functions[__aterminal_original_precmd]=$functions[precmd]
  precmd() { __aterminal_code=$?; __aterminal_original_precmd "$@"; }
else
  __aterminal_capture() { __aterminal_code=$?; return "$__aterminal_code"; }
  precmd_functions=(__aterminal_capture ${precmd_functions:#__aterminal_capture})
fi
__aterminal_precmd() { __aterminal_emit prompt "$__aterminal_code"; return "$__aterminal_code"; }
precmd_functions=(${precmd_functions:#__aterminal_precmd} __aterminal_precmd)
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
  $ok = $?; $nativeCode = $global:LASTEXITCODE
  $entry = Get-History -Count 1
  $seq = 0; $line = ''; $code = 'unknown'
  if ($entry) {
    $seq = $entry.Id; $line = $entry.CommandLine
    $tokens = $null; $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($line, [ref]$tokens, [ref]$errors)
    $statements = $ast.EndBlock.Statements
    if ($errors.Count -eq 0 -and $statements.Count -eq 1 -and $statements[0] -is [System.Management.Automation.Language.PipelineAst] -and -not $statements[0].Background -and $statements[0].PipelineElements.Count -eq 1) {
      $node = $statements[0].PipelineElements[0]
      if ($node -is [System.Management.Automation.Language.CommandAst]) {
        $name = $node.GetCommandName()
        $resolved = $(if ($name) { Get-Command $name -ErrorAction SilentlyContinue })
        if ($resolved -and $resolved.CommandType -eq 'Application') { $code = $nativeCode }
        elseif ($resolved -and $resolved.CommandType -eq 'Cmdlet') { $code = $(if ($ok) { 0 } else { 1 }) }
        # Aliases, functions, dynamic invocation and compound syntax cannot reuse
        # LASTEXITCODE from a different native command; keep their code unknown.
      }
    } else { $code = 'unknown' }
  }
  $value = 'prompt' + [char]0 + [string]$code + [char]0 + $PWD.Path + [char]0 + [string]$seq + [char]0 + $line + [char]0 + 'powershell:1' + [char]0
  [System.IO.File]::WriteAllText($env:ATERMINAL_SHELL_STATE + '.tmp', $value, [System.Text.UTF8Encoding]::new($false))
  Move-Item -LiteralPath ($env:ATERMINAL_SHELL_STATE + '.tmp') -Destination $env:ATERMINAL_SHELL_STATE -Force
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
        // Metadata and content must come from the same inode across atomic hook replacement.
        let file = std::fs::File::open(self.directory.join("state")).ok()?;
        let reported = file
            .metadata()
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        let reported_at = reported.as_millis();
        let reported_at_ns = u64::try_from(reported.as_nanos()).ok()?;
        let mut bytes = Vec::new();
        file.take(32769).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 32768 || bytes.last() != Some(&0) {
            return None;
        }
        let parts = bytes.split(|b| *b == 0).collect::<Vec<_>>();
        if parts.len() != 7 {
            return None;
        }
        let phase = std::str::from_utf8(parts[0]).ok()?;
        if !["prompt", "running"].contains(&phase) {
            return None;
        }
        let (dialect, association) = std::str::from_utf8(parts[5]).ok()?.split_once(':')?;
        if !["bash", "zsh", "powershell"].contains(&dialect) || !["0", "1"].contains(&association) {
            return None;
        }
        Some(
            json!({"phase":phase,"cwd":std::str::from_utf8(parts[2]).ok()?,"exit_code":if phase=="prompt"{std::str::from_utf8(parts[1]).ok()?.parse::<i32>().ok()}else{None},"evidence_source":"session_shell_hook","reported_at":reported_at,"reported_at_ns":reported_at_ns,"trusted_for_authorization":false,"sequence":std::str::from_utf8(parts[3]).ok()?.parse::<u64>().ok()?,"command":std::str::from_utf8(parts[4]).ok()?,"dialect":dialect,"command_association":association=="1","instance":self.directory.file_name()?.to_str()?}),
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

#[cfg(all(test, unix))]
mod toolset_tests {
    use super::*;
    use portable_pty::{Child, MasterPty, PtySize, native_pty_system};
    use std::{
        io::Write,
        thread,
        time::{Duration, Instant},
    };
    struct ShellFixture {
        integration: Integration,
        writer: Box<dyn Write + Send>,
        child: Box<dyn Child + Send + Sync>,
        _master: Box<dyn MasterPty + Send>,
        _temp: tempfile::TempDir,
    }
    impl ShellFixture {
        fn new(shell: &str, rc: &str) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("config");
            std::fs::create_dir(&original).unwrap();
            // Ubuntu's global completion audit can prompt before our isolated rc.
            // This fixture tests shell hooks, without loading system completions.
            if shell.ends_with("zsh") {
                std::fs::write(original.join(".zshenv"), "skip_global_compinit=1\n").unwrap();
            }
            std::fs::write(
                original.join(if shell.ends_with("zsh") {
                    ".zshrc"
                } else {
                    ".bashrc"
                }),
                rc,
            )
            .unwrap();
            let (integration, mut command) =
                Integration::prepare(temp.path(), &[shell.into()]).unwrap();
            command.env("HOME", &original);
            command.env("ATERMINAL_ORIGINAL_ZDOTDIR", &original);
            command.env("TERM", "xterm-256color");
            command.cwd(temp.path());
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let child = pair.slave.spawn_command(command).unwrap();
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader().unwrap();
            thread::spawn(move || {
                let _ = std::io::copy(&mut reader, &mut std::io::sink());
            });
            let writer = pair.master.take_writer().unwrap();
            let fixture = Self {
                integration,
                writer,
                child,
                _master: pair.master,
                _temp: temp,
            };
            fixture.wait(0);
            fixture
        }
        fn wait(&self, sequence: u64) -> Value {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let observed = self.integration.observation();
                if let Some(value) = &observed
                    && value["phase"] == "prompt"
                    && value["sequence"] == sequence
                {
                    return value.clone();
                }
                assert!(
                    Instant::now() < end,
                    "shell hook timed out at sequence {sequence}: {observed:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
        fn run(&mut self, command: &str, sequence: u64) -> Value {
            self.writer
                .write_all(format!("{command}\r").as_bytes())
                .unwrap();
            self.writer.flush().unwrap();
            self.wait(sequence)
        }
    }
    impl Drop for ShellFixture {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    #[test]
    fn bash_hooks_correlate_compound_exit_and_preserve_prompt_commands() {
        let mut fixture = ShellFixture::new(
            "/bin/bash",
            "PS1='fixture> '; HISTCONTROL=; HISTIGNORE=; PROMPT_COMMAND='true'\n",
        );
        let first = fixture.run("false", 1);
        assert_eq!(first["command"], "false");
        assert_eq!(first["exit_code"], 1);
        assert_eq!(first["command_association"], true);
        let second = fixture.run("true; false", 2);
        assert_eq!(second["command"], "true; false");
        assert_eq!(second["exit_code"], 1);
        let third = fixture.run("cd /", 3);
        assert_eq!(third["cwd"], "/");
        assert_eq!(third["exit_code"], 0);
        assert_eq!(third["trusted_for_authorization"], false);
        assert!(third["reported_at_ns"].as_u64().is_some());
    }
    #[test]
    fn zsh_hooks_preserve_existing_precmd_and_exact_exit() {
        let mut fixture = ShellFixture::new(
            "/bin/zsh",
            "PS1='fixture> '; precmd() { true; }; existing_hook() { true; }; precmd_functions=(existing_hook)\n",
        );
        let first = fixture.run("false", 1);
        assert_eq!(first["command"], "false");
        assert_eq!(first["exit_code"], 1);
        assert_eq!(first["command_association"], true);
        let second = fixture.run("true; false", 2);
        assert_eq!(second["command"], "true; false");
        assert_eq!(second["exit_code"], 1);
        let third = fixture.run("cd /", 3);
        assert_eq!(third["cwd"], "/");
        assert_eq!(third["exit_code"], 0);
    }
    #[test]
    fn bash_existing_debug_trap_is_preserved_and_association_is_unavailable() {
        let mut fixture = ShellFixture::new(
            "/bin/bash",
            "PS1='fixture> '; trap 'printf x >> \"$HOME/debug_marker\"' DEBUG\n",
        );
        assert_eq!(fixture.wait(0)["command_association"], false);
        fixture.writer.write_all(b"false\r").unwrap();
        fixture.writer.flush().unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(fixture._temp.path().join("config/debug_marker").exists());
        assert_eq!(fixture.wait(0)["command_association"], false);
    }
    #[test]
    fn malformed_or_truncated_hook_files_are_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("hook");
        std::fs::create_dir(&directory).unwrap();
        let integration = Integration {
            directory: directory.clone(),
        };
        for invalid in [
            b"prompt\0zero\0/\0".as_slice(),
            b"prompt\x000\0/\x000\0false\0bash:1".as_slice(),
        ] {
            std::fs::write(directory.join("state"), invalid).unwrap();
            assert!(integration.observation().is_none());
        }
        std::fs::write(
            directory.join("state"),
            b"prompt\x001\0/\x004\0false\0bash:1\0",
        )
        .unwrap();
        assert_eq!(integration.observation().unwrap()["exit_code"], 1);
    }
}
