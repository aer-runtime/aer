use super::{KillHandle, OsHandle, OsProcess};
use crate::AerError;
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// RAII wrapper for a Windows Job Object handle.
/// Drop calls CloseHandle, which triggers KILL_ON_JOB_CLOSE for any surviving
/// descendants. Using Arc<JobHandle> ensures the handle stays alive as long as
/// any thread holds a KillHandle reference, preventing handle-value recycling.
pub(crate) struct JobHandle(*mut c_void);

// SAFETY: Windows HANDLEs are per-process, not per-thread. Passing the same
// HANDLE across threads within the same process is safe and is the documented
// usage pattern for job objects shared between the main and monitor threads.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) struct WindowsProcess;

impl OsProcess for WindowsProcess {
    fn spawn(program: &str, args: &[&str]) -> Result<OsHandle, AerError> {
        // Create the job object first and wrap it immediately so all subsequent
        // error paths clean up via Drop — no manual CloseHandle calls needed.
        let raw_job = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if raw_job.is_null() {
            return Err(AerError::SpawnFailed(io::Error::last_os_error()));
        }
        let job = Arc::new(JobHandle(raw_job));

        // Configure kill-on-close: when the last handle to the job closes,
        // every process still in the job is terminated.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(AerError::SpawnFailed(io::Error::last_os_error()));
        }

        let child = Command::new(program)
            .args(args)
            // Pipes are required even though output is not surfaced to callers.
            // Without draining, a child writing beyond the OS pipe buffer deadlocks
            // wait_with_output(). Never use Stdio::inherit here.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(AerError::SpawnFailed)?;

        // Assign the child to the job. child.as_raw_handle() returns the process
        // HANDLE (*mut c_void), which AssignProcessToJobObject accepts directly.
        if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } == 0 {
            return Err(AerError::SpawnFailed(io::Error::last_os_error()));
        }

        let pid = child.id();
        Ok(OsHandle {
            pid,
            child,
            kill: KillHandle { job },
        })
    }

    fn wait(handle: OsHandle) -> Result<i32, AerError> {
        let OsHandle {
            mut child, kill, ..
        } = handle;

        // Drain pipes in a background thread so the root process can write freely
        // without filling the OS pipe buffer (which would deadlock child.wait()).
        // MUST start before child.wait() is called.
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

        // Wait for the root process only — NOT for grandchildren to close the pipe.
        let status = child.wait().map_err(AerError::WaitFailed)?;

        // Decrement the Arc<JobHandle> ref count. In the no-timeout path, task.rs
        // deliberately holds no extra clone, so this is the last reference:
        // CloseHandle fires immediately, triggering KILL_ON_JOB_CLOSE for every
        // grandchild still in the job (closing their inherited pipe handles and
        // unblocking the drain thread). In the timeout path, TerminateJobObject
        // has already killed the tree; this just decrements from 2 → 1 (the
        // monitor still holds one ref, which drops when the monitor thread exits).
        drop(kill);

        // Drain thread unblocks once all pipe write-ends are closed — either by
        // KILL_ON_JOB_CLOSE fired above, or by TerminateJobObject (timeout path)
        // which fires before child.wait() returns.
        let _ = drain.join();

        Ok(status.code().unwrap_or(-1))
    }

    fn kill_escalating(kill: KillHandle, _grace: Duration) -> Result<(), AerError> {
        // TerminateJobObject kills every process in the job simultaneously.
        // This closes all inherited pipe handles, which unblocks wait_with_output()
        // on the main thread. On Windows there is no graceful kill; _grace is ignored.
        if unsafe { TerminateJobObject(kill.job.0, 1) } == 0 {
            return Err(AerError::KillFailed(io::Error::last_os_error()));
        }
        Ok(())
    }
}
