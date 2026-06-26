use crate::AerError;
use std::process::Child;
use std::time::Duration;

/// Handle to a live OS process.
pub(crate) struct OsHandle {
    pub(crate) pid: u32,
    pub(crate) child: Child,
}

/// Platform abstraction for spawning, waiting on, and killing a process.
/// Implementations must not leak OS-specific behavior into callers.
pub(crate) trait OsProcess {
    fn spawn(program: &str, args: &[&str]) -> Result<OsHandle, AerError>;
    /// Blocks until the process exits. Returns the exit code.
    /// Returns -1 if the OS provides no exit code (e.g. signal-killed on Unix).
    fn wait(handle: OsHandle) -> Result<i32, AerError>;
    /// Sends a graceful termination signal, waits `grace`, then force-kills.
    /// On Unix: SIGTERM → sleep(grace) → SIGKILL.
    /// On Windows: TerminateProcess immediately (grace is ignored).
    fn kill_escalating(pid: u32, grace: Duration) -> Result<(), AerError>;
}

#[cfg(not(target_os = "windows"))]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "windows"))]
pub(crate) use unix::UnixProcess as PlatformProcess;
#[cfg(target_os = "windows")]
pub(crate) use windows::WindowsProcess as PlatformProcess;
