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
| Killed by signal (Unix) | `-1` (M1 sentinel; future milestones may use `-signal_number`) |
| OS provides no exit code | `-1` |

---

## 3. Ordering Invariants

These invariants are enforced by the state machine and validated by integration tests. All are required to hold in every milestone.

1. **Started precedes Exited.** `Started` is always the first event; `Exited` is always the last.
2. **Exactly one Started per run.** A successful `Task::run()` emits `Started` exactly once.
3. **Exactly one Exited per run.** A successful `Task::run()` emits `Exited` exactly once.
4. **No events on spawn failure.** If the OS refuses to spawn the process, neither `Started` nor `Exited` is emitted and `run()` returns an error.
5. **Exited is terminal.** No event is emitted after `Exited`.

---

## 4. Execution Semantics (M1)

- **Single-shot only.** One `Task::run()` call = one process execution. No reuse.
- **Synchronous.** `run()` blocks until the process exits.
- **Byte-level I/O.** stdout/stderr are captured internally to prevent pipe-buffer deadlock. They are not surfaced to callers in M1.
- **No PTY/terminal emulation.**

---

## 5. Milestone Definitions

| Milestone | Adds | Status |
|---|---|---|
| M1 | Core scaffold, state machine, STARTED/EXITED events, single-shot execution | ✓ Complete |
| M2 | Timeout handling | Pending |
| M3 | Kill escalation (SIGTERM → SIGKILL / TerminateProcess) | Pending |
| M4 | Process tree cleanup (Job Objects on Windows, setsid on Unix) | Pending |
| M5 | FFI boundary (C-compatible ABI) | Pending |
| M6 | .NET binding (P/Invoke wrapper) | Pending |
| M7 | Python binding (ctypes/cffi wrapper) | Pending |

---

## 6. Behavioral Invariants (design targets for future milestones)

The following invariants are not yet enforced in M1 but the code must be structured to eventually enforce them:

- No child process survives final termination (M4).
- No event is emitted after the terminal state (already structurally guaranteed by M1 state machine).
- No duplicate terminal events per task (already structurally guaranteed by M1 state machine).
