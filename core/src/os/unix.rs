use super::{KillHandle, OsHandle, OsProcess};
use crate::AerError;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub(crate) struct UnixProcess;

impl OsProcess for UnixProcess {
    fn spawn(program: &str, args: &[&str]) -> Result<OsHandle, AerError> {
        let child = Command::new(program)
            .args(args)
            // Pipes are required even though output is not surfaced to callers.
            // Without draining, a child writing beyond the OS pipe buffer deadlocks
            // wait_with_output(). Never use Stdio::inherit here.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // setsid() makes the child the leader of a new session and process group.
            // After exec, the child's PID == its PGID, so killpg(child_pid, sig)
            // broadcasts to the entire process tree.
            .pre_exec(|| {
                if unsafe { libc::setsid() } < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            })
            .spawn()
            .map_err(AerError::SpawnFailed)?;

        let pid = child.id();
        Ok(OsHandle {
            pid,
            child,
            kill: KillHandle { pgid: pid },
        })
    }

    fn wait(handle: OsHandle) -> Result<i32, AerError> {
        // wait_with_output() drains stdout+stderr before returning, preventing
        // the pipe-buffer deadlock described in spawn(). Output is discarded.
        // status.code() returns None if the process was killed by a signal;
        // -1 is the sentinel for "no exit code available."
        let output = handle
            .child
            .wait_with_output()
            .map_err(AerError::WaitFailed)?;
        Ok(output.status.code().unwrap_or(-1))
    }

    fn kill_escalating(kill: KillHandle, grace: Duration) -> Result<(), AerError> {
        // killpg broadcasts to the entire process group. After setsid, the child's
        // PGID == its PID, so kill.pgid == the pid passed to spawn.
        // SIGTERM: polite request; gives the group a chance to clean up.
        if unsafe { libc::killpg(kill.pgid as i32, libc::SIGTERM) } != 0 {
            return Err(AerError::KillFailed(io::Error::last_os_error()));
        }
        thread::sleep(grace);
        // SIGKILL: cannot be caught or ignored. ESRCH means the group is already
        // gone (responded to SIGTERM) — that is not an error.
        if unsafe { libc::killpg(kill.pgid as i32, libc::SIGKILL) } != 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                return Err(AerError::KillFailed(e));
            }
        }
        Ok(())
    }
}
