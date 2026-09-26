use anyhow::{Context, Result, bail};
use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
};

pub enum Output {
    Bytes(Vec<u8>),
    End,
    Error(String),
}
pub struct Session {
    pub output: Receiver<Output>,
    writer: Option<SyncSender<Vec<u8>>>,
    writer_error: Receiver<String>,
    child: Box<dyn Child + Send + Sync>,
    pub process_identity: String,
    shell: Option<crate::shell::Integration>,
    master: Option<Box<dyn MasterPty + Send>>,
}
impl Session {
    pub fn spawn(command: &[OsString], cwd: Option<&Path>, rows: u16, cols: u16) -> Result<Self> {
        Self::spawn_integrated(command, cwd, rows, cols, None)
    }
    pub(crate) fn spawn_integrated(
        command: &[OsString],
        cwd: Option<&Path>,
        rows: u16,
        cols: u16,
        integration: Option<&Path>,
    ) -> Result<Self> {
        ai_terminal_engine::check_size(rows, cols)?;
        let pair = native_pty_system()
            .openpty(size(rows, cols))
            .context("open PTY")?;
        let prepared = integration
            .map(|root| crate::shell::Integration::prepare(root, command))
            .transpose()?;
        let (shell, prepared) = if let Some((shell, cmd)) = prepared {
            (Some(shell), Some(cmd))
        } else {
            (None, None)
        };
        let mut cmd = if let Some(cmd) = prepared {
            cmd
        } else if let Some(executable) = command.first() {
            let mut cmd = CommandBuilder::new(executable);
            cmd.args(&command[1..]);
            cmd
        } else {
            let mut cmd = CommandBuilder::new(default_shell());
            if !cfg!(windows) {
                cmd.arg("-i");
            }
            cmd
        };
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        // P0 baseline only: capability/terminfo conformance is a release gate.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("start shell or command")?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (out_tx, output) = mpsc::sync_channel(32);
        thread::Builder::new()
            .name("pty-read".into())
            .spawn(move || {
                let mut buffer = [0; 32768];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            let _ = out_tx.send(Output::End);
                            break;
                        }
                        // Keep draining after the consumer goes away so ConPTY can close.
                        Ok(n) => {
                            let _ = out_tx.send(Output::Bytes(buffer[..n].to_vec()));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => {
                            let _ = out_tx.send(Output::Error(e.to_string()));
                            break;
                        }
                    }
                }
            })?;
        let (input_tx, input_rx) = mpsc::sync_channel::<Vec<u8>>(128);
        let (err_tx, writer_error) = mpsc::channel();
        thread::Builder::new()
            .name("pty-write".into())
            .spawn(move || {
                while let Ok(bytes) = input_rx.recv() {
                    if let Err(e) = writer.write_all(&bytes).and_then(|_| writer.flush()) {
                        let _ = err_tx.send(e.to_string());
                        break;
                    }
                }
            })?;
        let process_identity = child
            .process_id()
            .and_then(crate::process::identity)
            .unwrap_or_default();
        Ok(Self {
            output,
            process_identity,
            shell,
            writer: Some(input_tx),
            writer_error,
            child,
            master: Some(pair.master),
        })
    }
    pub fn shell_observation(&self) -> Option<serde_json::Value> {
        self.shell.as_ref()?.observation()
    }
    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }
    pub fn foreground_group(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.master
                .as_ref()?
                .process_group_leader()
                .and_then(|p| u32::try_from(p).ok())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
    pub fn write(&self, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > 1024 * 1024 {
            bail!("paste exceeds the prototype's 1 MiB limit")
        }
        self.writer
            .as_ref()
            .context("session closed")?
            .try_send(bytes)
            .context("input queue full or closed; input was not accepted")
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master
            .as_ref()
            .context("session closed")?
            .resize(size(rows, cols))
    }
    pub fn check_writer(&self) -> Result<()> {
        if let Ok(e) = self.writer_error.try_recv() {
            bail!("PTY write failed: {e}")
        }
        Ok(())
    }
    pub fn exit_status(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self.child.try_wait()?)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let (_, empty) = mpsc::channel();
        drop(std::mem::replace(&mut self.output, empty));
        self.writer.take();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        // ConPTY's independent reader must stay alive during teardown.
        self.master.take();
        let _ = self.child.wait();
    }
}
fn size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(unix)]
pub(crate) fn default_shell() -> OsString {
    std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into())
}
#[cfg(windows)]
pub(crate) fn default_shell() -> OsString {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("pwsh.exe");
            if candidate.is_file() {
                return candidate.into_os_string();
            }
        }
    }
    "powershell.exe".into()
}
