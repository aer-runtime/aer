use std::fmt;

/// Events emitted during a single task execution.
/// `Started` always precedes `Exited`; no other ordering is valid.
/// `StdoutChunk`/`StderrChunk` are emitted only when `CaptureOutput` is enabled
/// and always appear strictly between `Started` and `Exited`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Emitted immediately after the OS confirms process spawn.
    Started { pid: u32 },
    /// Emitted after the process exits. `code` is -1 if the OS provides no exit code
    /// (e.g. signal-killed on Unix).
    Exited { code: i32 },
    /// A chunk of bytes from the process's stdout pipe. `seq` is monotonically
    /// increasing within the stdout stream only; no cross-stream ordering is implied.
    /// Only emitted when `Task::with_capture_output(true)` is set.
    StdoutChunk { seq: u64, bytes: Vec<u8> },
    /// A chunk of bytes from the process's stderr pipe. `seq` is monotonically
    /// increasing within the stderr stream only; no cross-stream ordering is implied.
    /// Only emitted when `Task::with_capture_output(true)` is set.
    StderrChunk { seq: u64, bytes: Vec<u8> },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Started { pid } => write!(f, "Started(pid={})", pid),
            Event::Exited { code } => write!(f, "Exited(code={})", code),
            Event::StdoutChunk { seq, bytes } => {
                write!(f, "StdoutChunk(seq={}, len={})", seq, bytes.len())
            }
            Event::StderrChunk { seq, bytes } => {
                write!(f, "StderrChunk(seq={}, len={})", seq, bytes.len())
            }
        }
    }
}
