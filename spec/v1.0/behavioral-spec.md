# AER Behavioral Specification — v1.0

This document is the authoritative definition of what AER guarantees. Code is derived from this; this is not derived from code.

---

## 1. State Machine

```
Created ──spawn──▶ Running ──exit──▶ Exited
```

**Rules:**
- Transitions are strictly one-directional. No backward transitions, no self-transitions.
- `Created` is the initial state of every task execution.
- `Exited` is the only terminal state. No transitions out of `Exited` are valid.
- Invalid transitions are explicit errors, not silently ignored.

| From | To | Trigger | Valid |
|---|---|---|---|
| Created | Running | OS confirms spawn | ✓ |
| Running | Exited | OS confirms process termination | ✓ |
| Any | Any other | — | ✗ error |

---

## 2. Event Model

Events are the observable output of a task execution. The state machine is internal; events are the external contract.

| Event | Trigger | Fields | Guaranteed ordering |
|---|---|---|---|
| `Started` | Immediately after OS confirms spawn | `pid: u32` | Always before `Exited` |
| `Exited` | After OS confirms process termination | `code: i32` | Always after `Started` |

### Exit code mapping

| Condition | `code` value |
|---|---|
| Normal exit | OS exit code (0–255 on Unix; 0–4294967295 on Windows, stored as i32) |
| Killed by signal (Unix) | `-1` (sentinel; future milestones may use `-signal_number`) |
| Killed by timeout | `-1` |
| OS provides no exit code | `-1` |

---

## 3. Ordering Invariants

These invariants are enforced by the state machine and validated by integration tests. All are required to hold in every milestone.

1. **Started precedes Exited.** `Started` is always the first event; `Exited` is always the last.
2. **Exactly one Started per run.** A successful `Task::run()` emits `Started` exactly once.
3. **Exactly one Exited per run.** A successful `Task::run()` emits `Exited` exactly once.
4. **No events on spawn failure.** If the OS refuses to spawn the process, neither `Started` nor `Exited` is emitted and `run()` returns an error.
5. **Exited is terminal.** No event is emitted after `Exited`.
6. **Exited fires even on timeout.** If the process is killed due to a timeout, `Exited { code: -1 }` is still emitted before `run()` returns `Err(TimedOut)`.

---

## 4. Execution Semantics

- **Single-shot only.** One `Task::run()` call = one process execution. No reuse.
- **Synchronous.** `run()` blocks until the process exits.
- **Byte-level I/O.** stdout/stderr are captured internally to prevent pipe-buffer deadlock. They are not surfaced to callers in M1 or M2.
- **No PTY/terminal emulation.**
- **Optional timeout.** Set via `Task::with_timeout(Duration)`. When not set, `run()` blocks indefinitely (M1 behavior).

---

## 5. Timeout Semantics (M2)

### Configuration

```rust
let task = Task::new("my-program", vec![])
    .with_timeout(Duration::from_secs(30));
```

`with_timeout` is a builder method. Tasks without it behave identically to M1.

### Kill sequence

When the timeout elapses and the process has not yet exited:

| Platform | Sequence |
|---|---|
| Unix | SIGTERM → wait 5 seconds → SIGKILL |
| Windows | `TerminateProcess` immediately |

The 5-second grace window on Unix gives the process a chance to handle SIGTERM and exit cleanly. SIGKILL is sent unconditionally after the grace period regardless of whether the process responded to SIGTERM. On Windows there is no reliable graceful kill for arbitrary console processes; `TerminateProcess` is used directly.

### Return value on timeout

`run()` returns `Err(AerError::TimedOut)` after emitting `Exited`. The `Started → Exited` invariant is preserved even when the process is killed.

### New error variants (M2)

| Variant | Meaning |
|---|---|
| `TimedOut` | Process was killed because the timeout elapsed |
| `KillFailed(io::Error)` | The kill attempt itself failed (rare; process may have already exited) |

---

## 6. Milestone Definitions

| Milestone | Adds | Status |
|---|---|---|
| M1 | Core scaffold, state machine, STARTED/EXITED events, single-shot execution | ✓ Complete |
| M2 | Configurable timeout, kill escalation (SIGTERM → SIGKILL / TerminateProcess) | In progress |
| M3 | Process tree cleanup (Job Objects on Windows, setsid on Unix) | Pending |
| M4 | FFI boundary (C-compatible ABI) | Pending |
| M5 | .NET binding (P/Invoke wrapper) | Pending |
| M6 | Python binding (ctypes/cffi wrapper) | Pending |

---

## 7. Behavioral Invariants (design targets for future milestones)

The following invariants are not yet enforced but the code must be structured to eventually enforce them:

- No child process survives final termination (M3).
- No event is emitted after the terminal state (already structurally guaranteed by M1 state machine).
- No duplicate terminal events per task (already structurally guaranteed by M1 state machine).
