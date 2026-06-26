use crate::{
    event::Event,
    machine::{State, StateMachine},
    os::{OsProcess, PlatformProcess},
    AerError,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

const KILL_GRACE: Duration = Duration::from_secs(5);

/// A single-shot task: one command, one execution, deterministic lifecycle.
pub struct Task {
    program: String,
    args: Vec<String>,
    timeout: Option<Duration>,
}

impl Task {
    pub fn new(program: impl Into<String>, args: Vec<impl Into<String>>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: None,
        }
    }

    /// Sets a maximum wall-clock duration for the process. If the process has not
    /// exited by the deadline, it is killed via platform-appropriate escalation
    /// and `run()` returns `Err(AerError::TimedOut)`.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Spawns the process and blocks until it exits.
    ///
    /// Calls `on_event` exactly twice on success: `Started` then `Exited`.
    /// If spawning fails, `on_event` is never called and `SpawnFailed` is returned.
    /// If a timeout was set and fires, `Exited` is still called before returning
    /// `Err(AerError::TimedOut)`.
    pub fn run(&self, mut on_event: impl FnMut(Event)) -> Result<(), AerError> {
        let mut machine = StateMachine::new();

        let str_args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let handle = PlatformProcess::spawn(&self.program, &str_args)?;

        let pid = handle.pid;
        machine.transition(State::Running)?;
        on_event(Event::Started { pid });

        // If a timeout is configured, spawn a monitor thread that kills the process
        // tree after the deadline. The main thread cancels it by sending on `cancel_tx`
        // once wait() returns. The `timed_out` flag distinguishes a timeout kill
        // from a natural exit.
        //
        // The KillHandle is cloned ONLY when the monitor thread is spawned. This
        // ensures wait() holds the sole Arc reference in the no-timeout path, so
        // drop(kill) inside wait() fires CloseHandle (Windows) immediately after
        // the root exits — unblocking any grandchildren that hold inherited pipes.
        let timed_out = Arc::new(AtomicBool::new(false));
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>();

        if let Some(timeout) = self.timeout {
            let kill_for_monitor = handle.kill.clone();
            let timed_out_clone = Arc::clone(&timed_out);
            thread::spawn(move || {
                if let Err(mpsc::RecvTimeoutError::Timeout) = cancel_rx.recv_timeout(timeout) {
                    timed_out_clone.store(true, Ordering::SeqCst);
                    let _ = PlatformProcess::kill_escalating(kill_for_monitor, KILL_GRACE);
                }
            });
        }

        let os_code = PlatformProcess::wait(handle)?;
        let _ = cancel_tx.send(()); // wake monitor early if process exited before timeout

        let was_timed_out = timed_out.load(Ordering::SeqCst);
        // Enforce the spec: timeout kills always report code -1 regardless of what
        // the OS returns (TerminateProcess on Windows sets an explicit exit code;
        // SIGKILL on Unix produces no code at all). The caller learns the real reason
        // from the Err(TimedOut) return value, not from the exit code.
        let code = if was_timed_out { -1 } else { os_code };

        machine.transition(State::Exited)?;
        on_event(Event::Exited { code });

        if was_timed_out {
            return Err(AerError::TimedOut);
        }

        Ok(())
    }
}
