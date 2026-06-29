use crate::{
    event::{Event, ExitReason},
    machine::{State, StateMachine},
    os::{KillHandle, OsProcess, OutputSinks, PlatformProcess},
    AerError,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const KILL_GRACE: Duration = Duration::from_secs(5);

/// A handle that allows an external caller to cancel a running task.
///
/// Obtained before `run_with_cancel` is called. The handle can be cloned and
/// shared across threads. Calling `cancel()` is idempotent — only the first
/// call has effect. Calling `cancel()` before the process has started or after
/// it has already exited is safe and has no effect on the reported exit reason.
#[derive(Clone)]
pub struct CancelHandle {
    /// Set true the first time cancel() is called.
    cancelled: Arc<AtomicBool>,
    /// Set true only when cancel() actually fired a kill while the process was live.
    /// Used by run_impl to determine whether exit reason is CancelRequested.
    kill_fired: Arc<AtomicBool>,
    /// Non-None only between spawn and wait() returning. Mutex guards concurrent cancel().
    kill: Arc<Mutex<Option<KillHandle>>>,
}

impl Default for CancelHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelHandle {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            kill_fired: Arc::new(AtomicBool::new(false)),
            kill: Arc::new(Mutex::new(None)),
        }
    }

    /// Request cancellation. If the process is running, it is killed immediately
    /// using the same escalation as a timeout kill. If the process has not started
    /// yet or has already exited, this is a no-op (the exit reason is unaffected).
    ///
    /// Thread-safe. Only the first call has effect; subsequent calls are no-ops.
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::SeqCst) {
            if let Ok(guard) = self.kill.lock() {
                if let Some(kill) = guard.as_ref() {
                    self.kill_fired.store(true, Ordering::SeqCst);
                    let _ = PlatformProcess::kill_escalating(kill.clone(), KILL_GRACE);
                }
                // kill is None: called before spawn or after wait() returned.
                // kill_fired stays false; run_impl will not report CancelRequested.
            }
        }
    }

    /// Returns true if `cancel()` has been called at least once.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A single-shot task: one command, one execution, deterministic lifecycle.
pub struct Task {
    program: String,
    args: Vec<String>,
    timeout: Option<Duration>,
    capture_output: bool,
}

impl Task {
    pub fn new(program: impl Into<String>, args: Vec<impl Into<String>>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: None,
            capture_output: false,
        }
    }

    /// Sets a maximum wall-clock duration for the process. If the process has not
    /// exited by the deadline, it is killed and `run()` returns `Err(AerError::TimedOut)`.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Enables stdout and stderr capture. When true, `StdoutChunk` and
    /// `StderrChunk` events are emitted between `Started` and `Exited`.
    pub fn with_capture_output(mut self, capture: bool) -> Self {
        self.capture_output = capture;
        self
    }

    /// Spawns the process and blocks until it exits.
    ///
    /// Events emitted: `Started`, optional chunks (when capture enabled), then `Exited`.
    /// If spawning fails, `on_event` is never called and `SpawnFailed` is returned.
    pub fn run(&self, on_event: impl FnMut(Event)) -> Result<(), AerError> {
        self.run_impl(on_event, None)
    }

    /// Like `run`, but wires up a `CancelHandle` so an external caller can cancel
    /// the execution mid-flight. If cancelled, `Exited { reason: CancelRequested,
    /// code: -1 }` is emitted and `Err(AerError::Cancelled)` is returned.
    pub fn run_with_cancel<F: FnMut(Event)>(
        &self,
        on_event: F,
        cancel: &CancelHandle,
    ) -> Result<(), AerError> {
        self.run_impl(on_event, Some(cancel))
    }

    fn run_impl<F: FnMut(Event)>(
        &self,
        mut on_event: F,
        cancel: Option<&CancelHandle>,
    ) -> Result<(), AerError> {
        let mut machine = StateMachine::new();

        let str_args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let handle = PlatformProcess::spawn(&self.program, &str_args)?;

        let pid = handle.pid;
        machine.transition(State::Running)?;
        on_event(Event::Started { pid });

        // Wire the cancel handle to the live kill handle so cancel() can reach the process.
        // Also handle the case where cancel() was called before spawn completed:
        // in that case the flag is set but kill_fired is false; do the kill now.
        if let Some(ch) = cancel {
            *ch.kill.lock().unwrap() = Some(handle.kill.clone());
            if ch.cancelled.load(Ordering::SeqCst) && !ch.kill_fired.load(Ordering::SeqCst) {
                ch.kill_fired.store(true, Ordering::SeqCst);
                let _ = PlatformProcess::kill_escalating(handle.kill.clone(), KILL_GRACE);
            }
        }

        // Timeout monitor thread: kills the process tree after the deadline.
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

        let (sinks, stdout_rx, stderr_rx) = if self.capture_output {
            let (stdout_tx, stdout_rx) = mpsc::channel::<(u64, Vec<u8>)>();
            let (stderr_tx, stderr_rx) = mpsc::channel::<(u64, Vec<u8>)>();
            (
                OutputSinks {
                    stdout: Some(stdout_tx),
                    stderr: Some(stderr_tx),
                },
                Some(stdout_rx),
                Some(stderr_rx),
            )
        } else {
            (
                OutputSinks {
                    stdout: None,
                    stderr: None,
                },
                None,
                None,
            )
        };

        let os_code = PlatformProcess::wait(handle, sinks)?;

        // Disconnect the kill handle: process is dead, cancel() is now a no-op.
        if let Some(ch) = cancel {
            *ch.kill.lock().unwrap() = None;
        }

        let _ = cancel_tx.send(());

        let was_timed_out = timed_out.load(Ordering::SeqCst);
        // kill_fired is true only if cancel() actually killed the process while live.
        let was_cancelled = cancel
            .map(|ch| ch.kill_fired.load(Ordering::SeqCst))
            .unwrap_or(false);

        let reason = if was_timed_out {
            ExitReason::TimedOut
        } else if was_cancelled {
            ExitReason::CancelRequested
        } else {
            ExitReason::NaturalExit
        };

        let code = if was_timed_out || was_cancelled {
            -1
        } else {
            os_code
        };

        // Drain captured output: emitted between Started and Exited.
        if let Some(rx) = stdout_rx {
            for (seq, bytes) in rx {
                on_event(Event::StdoutChunk { seq, bytes });
            }
        }
        if let Some(rx) = stderr_rx {
            for (seq, bytes) in rx {
                on_event(Event::StderrChunk { seq, bytes });
            }
        }

        machine.transition(State::Exited)?;
        on_event(Event::Exited { code, reason });

        if was_timed_out {
            return Err(AerError::TimedOut);
        }
        if was_cancelled {
            return Err(AerError::Cancelled);
        }

        Ok(())
    }
}
