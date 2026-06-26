use super::{OsHandle, OsProcess};
use crate::AerError;
use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub(crate) struct UnixProcess;

impl OsProcess for UnixProcess {
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
        // status.code() returns None if the process was killed by a signal;
        // -1 is the sentinel for "no exit code available."
        let output = handle
            .child
            .wait_with_output()
            .map_err(AerError::WaitFailed)?;
        Ok(output.status.code().unwrap_or(-1))
    }

    fn kill_escalating(pid: u32, grace: Duration) -> Result<(), AerError> {
        // SIGTERM: polite request to exit; process can handle and clean up.
        if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
            return Err(AerError::KillFailed(io::Error::last_os_error()));
        }
        thread::sleep(grace);
        // SIGKILL: cannot be caught or ignored. ESRCH means the process already
        // exited (likely responded to SIGTERM) — that is not an error.
        if unsafe { libc::kill(pid as i32, libc::SIGKILL) } != 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                return Err(AerError::KillFailed(e));
            }
        }
        Ok(())
    }
}
