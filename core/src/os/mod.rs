use crate::AerError;
use std::process::Child;
use std::time::Duration;

/// Lightweight handle that the timeout monitor thread uses to kill the process tree.
///
/// Windows: wraps an `Arc<JobHandle>` so the OS handle stays alive until all
/// threads are done with it, preventing handle-value recycling races.
/// Unix: wraps the pgid (== pid after setsid); no heap allocation needed.
#[derive(Clone)]
pub(crate) struct KillHandle {
    #[cfg(windows)]
    pub(crate) job: std::sync::Arc<windows::JobHandle>,
    #[cfg(not(windows))]
    pub(crate) pgid: u32,
}

/// Handle to a live OS process.
pub(crate) struct OsHandle {
    pub(crate) pid: u32,
    pub(crate) child: Child,
    pub(crate) kill: KillHandle,
}

/// Platform abstraction for spawning, waiting on, and killing a process.
/// Implementations must not leak OS-specific behavior into callers.
pub(crate) trait OsProcess {
    fn spawn(program: &str, args: &[&str]) -> Result<OsHandle, AerError>;
    /// Blocks until the process exits. Returns the exit code.
    /// Returns -1 if the OS provides no exit code (e.g. signal-killed on Unix).
    fn wait(handle: OsHandle) -> Result<i32, AerError>;
    /// Kills the entire process tree. On Unix: SIGTERM → sleep(grace) → SIGKILL
    /// to the process group. On Windows: TerminateJobObject immediately.
    fn kill_escalating(kill: KillHandle, grace: Duration) -> Result<(), AerError>;
}

#[cfg(not(target_os = "windows"))]
mod unix;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(not(target_os = "windows"))]
pub(crate) use unix::UnixProcess as PlatformProcess;
#[cfg(target_os = "windows")]
pub(crate) use windows::WindowsProcess as PlatformProcess;
