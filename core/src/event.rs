use std::fmt;

/// Events emitted during a single task execution.
/// `Started` always precedes `Exited`; no other ordering is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Emitted immediately after the OS confirms process spawn.
    Started { pid: u32 },
    /// Emitted after the process exits. `code` is -1 if the OS provides no exit code
    /// (e.g. signal-killed on Unix).
    Exited { code: i32 },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Started { pid } => write!(f, "Started(pid={})", pid),
            Event::Exited { code } => write!(f, "Exited(code={})", code),
        }
    }
}
