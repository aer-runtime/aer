# AER Core — Implementation Plan

The behavioral spec (`spec/aer-core-behavioral-spec-v1.1.md`) is authoritative for what the system must guarantee. This document is authoritative for how we are getting there: which milestones exist, what is in scope for each, and where we currently stand.

---

## Milestones

### M1: Deterministic Spawn & Lifecycle Events ✓
Process spawn, `Started` / `Exited` events, state machine (`Created → Running → Exited`).

### M2: Timeout & Kill Escalation ✓
Configurable timeout; graceful → forceful kill escalation.

### M3: Process Tree Cleanup ✓
Kill the entire process tree, not just the root process. Windows Job Objects; Unix `setsid` + `killpg`.

### M4: Observation Tier & FFI Boundary ✓
`StdoutChunk` / `StderrChunk` events. On-demand cancellation via `CancelHandle`. C-compatible ABI (`aer.h`) with `aer_task_new`, `aer_task_run`, `aer_task_free`, `aer_cancel_new`, `aer_cancel_free`, `aer_cancel_request`.

### M5: .NET Binding
*P/Invoke wrapper over the M4 C FFI. Prerequisite for AER Flow.*

| Issue | Title | Depends on |
|---|---|---|
| #59 | Project scaffold & raw P/Invoke layer | — |
| #60 | Safe handles | #59 |
| #61 | Callback marshalling | #60 |
| #62 | High-level managed wrapper | #61 |
| #63 | Cancellation integration | #62 |
| #64 | Integration tests & docs | #63 |

**Current issue:** #59 (PR #65, pending CI).

**Acceptance criteria for M5 complete:** CI passes 100% on Windows and Linux for all six issues; AER Flow can reference `Aer.Core` and call `AerTask` without any direct P/Invoke.

### M6: Python Binding
Deferred — no consumer exists yet.

---

## Completed Milestones

M1, M2, M3, M4.

---

## Open Questions

None for M5 — the C ABI (`aer.h`) is frozen and the .NET binding is a mechanical translation of it.
