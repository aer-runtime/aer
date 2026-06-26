use crate::{
    event::Event,
    machine::{State, StateMachine},
    os::{OsProcess, PlatformProcess},
    AerError,
};

/// A single-shot task: one command, one execution, deterministic lifecycle.
pub struct Task {
    program: String,
    args: Vec<String>,
}

impl Task {
    pub fn new(program: impl Into<String>, args: Vec<impl Into<String>>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Spawns the process and blocks until it exits.
    ///
    /// Calls `on_event` exactly twice on success: `Started` then `Exited`.
    /// If spawning fails, `on_event` is never called and `SpawnFailed` is returned.
    pub fn run(&self, mut on_event: impl FnMut(Event)) -> Result<(), AerError> {
        let mut machine = StateMachine::new();

        let str_args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let handle = PlatformProcess::spawn(&self.program, &str_args)?;

        let pid = handle.pid;
        machine.transition(State::Running)?;
        on_event(Event::Started { pid });

        let code = PlatformProcess::wait(handle)?;
        machine.transition(State::Exited)?;
        on_event(Event::Exited { code });

        Ok(())
    }
}
