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
        let mut cmd = Command::new(program);
        cmd.args(args)
            // Pipes are required even though output is not surfaced to callers.
            // Without draining, a child writing beyond the OS pipe buffer deadlocks
            // child.wait(). Never use Stdio::inherit here.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // SAFETY: The closure only calls setsid(), which is documented as
        // async-signal-safe — safe to call between fork and exec.
        let child = unsafe {
            cmd.pre_exec(|| {
                // setsid() makes the child the leader of a new session and process
                // group. After exec, child PID == PGID, so killpg(child_pid, sig)
                // broadcasts to the entire process tree rooted here.
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            })
        }
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
        let OsHandle {
            mut child, kill, ..
        } = handle;
        let pgid = kill.pgid;

        // Drain pipes concurrently so the root process can write without blocking.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let drain = thread::spawn(move || {
            use std::io::{copy, sink};
            if let Some(mut out) = stdout {
                let _ = copy(&mut out, &mut sink());
            }
            if let Some(mut err) = stderr {
                let _ = copy(&mut err, &mut sink());
            }
        });

        // Wait for the root process only.
        let status = child.wait().map_err(AerError::WaitFailed)?;

        // Kill the entire process group after root exits. On the timeout path,
        // kill_escalating already sent SIGKILL; ESRCH (empty group) is not an error.
        // On the natural-exit path, this terminates any grandchildren that inherited
        // stdout/stderr handles, unblocking the drain thread below.
        if unsafe { libc::killpg(pgid as i32, libc::SIGKILL) } != 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                // Best-effort: do not lose the exit code over a cleanup failure.
            }
        }

        let _ = drain.join();

        Ok(status.code().unwrap_or(-1))
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
