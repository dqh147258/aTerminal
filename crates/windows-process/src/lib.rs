//! Narrow Windows process completion boundary for portable-pty 0.9.
#![cfg(windows)]

use portable_pty::{Child, ExitStatus, win::WinChild};
use std::{
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle},
};
use windows_sys::Win32::{
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{GetExitCodeProcess, WaitForSingleObject},
};

/// Observe an exit only after the process handle becomes signaled.
/// The child retains ownership of its handle throughout both OS calls.
#[allow(unsafe_code)]
pub fn try_wait(child: &mut WinChild) -> io::Result<Option<ExitStatus>> {
    let raw = child
        .as_raw_handle()
        .ok_or_else(|| io::Error::other("PTY child has no Windows process handle"))?;
    // SAFETY: this concrete WinChild owns the handle in a private OwnedHandle.
    // Its exclusive borrow keeps that owner alive throughout the poll. Do not
    // broaden this boundary to arbitrary implementations of Child/AsRawHandle.
    let process = unsafe { BorrowedHandle::borrow_raw(raw) };
    poll_process(process)
}

#[allow(unsafe_code)]
fn poll_process(process: BorrowedHandle<'_>) -> io::Result<Option<ExitStatus>> {
    // SAFETY: the borrowed handle stays owned throughout both calls. Neither
    // call takes ownership or closes it, and a zero timeout never blocks.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
        WAIT_OBJECT_0 => {}
        WAIT_TIMEOUT => return Ok(None),
        WAIT_FAILED => return Err(io::Error::last_os_error()),
        state => {
            return Err(io::Error::other(format!(
                "unexpected process wait state: {state}"
            )));
        }
    }
    let mut code = 0;
    // SAFETY: the same still-owned handle is now signaled; code is a valid out pointer.
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // Once signaled, even 259 is a real exit code, not the STILL_ACTIVE sentinel.
    Ok(Some(ExitStatus::with_exit_code(code)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::io::AsHandle,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn running_child_is_pending_and_signaled_child_can_exit_with_259() {
        let mut child = Command::new("cmd.exe")
            .args(["/d", "/c", "set /p gate= >nul & exit /b 259"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(poll_process(child.as_handle()).unwrap().is_none());
        // EOF releases set /p without depending on a startup delay.
        drop(child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = poll_process(child.as_handle()).unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("completed Windows child was not observed");
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.exit_code(), 259);
        assert_eq!(
            poll_process(child.as_handle())
                .unwrap()
                .unwrap()
                .exit_code(),
            259
        );
        assert_eq!(child.wait().unwrap().code(), Some(259));
    }
}
