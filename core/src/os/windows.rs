use super::{OsHandle, OsProcess};
use crate::AerError;
use std::io;
use std::process::{Command, Stdio};
use std::time::Duration;

pub(crate) struct WindowsProcess;

impl OsProcess for WindowsProcess {
    fn spawn(program: &str, args: &[&str]) -> Result<OsHandle, AerError> {
        let child = Command::new(program)
            .args(args)
            // Pipes are required even though M1 discards captured output.
            // Without draining, a child that writes more than the OS pipe buffer
            // will deadlock inside wait_with_output(). Never use Stdio::inherit here.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(AerError::SpawnFailed)?;

        let pid = child.id();
        Ok(OsHandle { pid, child })
    }

    fn wait(handle: OsHandle) -> Result<i32, AerError> {
        // wait_with_output() drains stdout+stderr before returning, preventing
        // the pipe-buffer deadlock described in spawn(). Output is discarded in M1/M2.
        let output = handle
            .child
            .wait_with_output()
            .map_err(AerError::WaitFailed)?;
        Ok(output.status.code().unwrap_or(-1))
    }

    fn kill_escalating(pid: u32, _grace: Duration) -> Result<(), AerError> {
        // Windows has no reliable graceful kill for arbitrary console processes.
        // TerminateProcess is used directly; _grace is accepted for API uniformity.
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };

        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            return Err(AerError::KillFailed(io::Error::last_os_error()));
        }
        let ok = unsafe { TerminateProcess(handle, 1) };
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return Err(AerError::KillFailed(io::Error::last_os_error()));
        }
        Ok(())
    }
}
