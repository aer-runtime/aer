use super::{OsHandle, OsProcess};
use crate::AerError;
use std::process::{Command, Stdio};

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
        // the pipe-buffer deadlock described in spawn(). Output is discarded in M1.
        // status.code() returns None if the process was killed by a signal;
        // -1 is the M1 sentinel for "no exit code available."
        let output = handle
            .child
            .wait_with_output()
            .map_err(AerError::WaitFailed)?;
        Ok(output.status.code().unwrap_or(-1))
    }
}
