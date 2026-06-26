use super::{KillHandle, OsHandle, OsProcess};
use crate::AerError;
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::Arc;
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
        // wait_with_output() drains stdout+stderr before returning.
        // When this call returns, handle is consumed and handle.kill (Arc<JobHandle>)
        // drops. If any children survived (natural-exit path), KILL_ON_JOB_CLOSE
        // fires at that point. On the timeout path, TerminateJobObject already killed
        // the tree; CloseHandle via Drop is then a no-op cleanup.
        let output = handle
            .child
            .wait_with_output()
            .map_err(AerError::WaitFailed)?;
        Ok(output.status.code().unwrap_or(-1))
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
