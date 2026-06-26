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
        // after the deadline. The main thread cancels it by sending on `cancel_tx`
        // once wait() returns. The `timed_out` flag distinguishes a timeout kill
        // from a natural exit.
        let timed_out = Arc::new(AtomicBool::new(false));
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>();

        if let Some(timeout) = self.timeout {
            let timed_out_clone = Arc::clone(&timed_out);
            thread::spawn(move || {
                if let Err(mpsc::RecvTimeoutError::Timeout) = cancel_rx.recv_timeout(timeout) {
                    timed_out_clone.store(true, Ordering::SeqCst);
                    let _ = PlatformProcess::kill_escalating(pid, KILL_GRACE);
                }
            });
        }

        let code = PlatformProcess::wait(handle)?;
        let _ = cancel_tx.send(()); // wake monitor early if process exited before timeout

        machine.transition(State::Exited)?;
        on_event(Event::Exited { code });

        if timed_out.load(Ordering::SeqCst) {
            return Err(AerError::TimedOut);
        }

        Ok(())
    }
}
