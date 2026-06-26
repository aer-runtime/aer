# AER — Claude Code Instructions

AER is a cross-language process supervision engine. The Rust crate `aer-core` is the product; all invariants — deterministic spawn, STARTED/EXITED events, state machine transitions — are enforced here. Language bindings (.NET, Python) are thin translation layers added in later milestones.

---

## Repo structure

```
aer/
├── core/          aer-core crate — the only place with process logic
│   ├── src/
│   │   ├── lib.rs        public API surface, AerError definition
│   │   ├── event.rs      Event enum (Started, Exited)
│   │   ├── machine.rs    StateMachine (Created → Running → Exited)
│   │   ├── task.rs       Task::run() — drives the machine and emits events
│   │   └── os/           platform abstraction (windows.rs / unix.rs)
│   └── tests/
│       └── integration_test.rs
├── spec/v1.0/     behavioral specification (source of truth, not code)
├── .github/workflows/
│   ├── ci.yml             lint + fmt + test on win + linux
│   └── release-please.yml versioning and changelog
└── pixi.toml      task runner and toolchain manager
```

---

## Running tasks

Always use `pixi run <task>`. Never invoke `cargo` directly in CI.

| Task | Command |
|---|---|
| `build` | `cargo build --workspace` |
| `test` | `cargo test --workspace` |
| `lint` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `fmt` | `cargo fmt --all` (fix) |
| `fmt-check` | `cargo fmt --all -- --check` (CI) |

Pixi manages the Rust toolchain — no separate `rustup` install needed.

---

## Module responsibilities

- **lib.rs** — re-exports public types, defines `AerError`. Nothing else.
- **event.rs** — pure data: the `Event` enum. No logic.
- **machine.rs** — `StateMachine` enforces legal transitions. Not public; callers see only events.
- **task.rs** — `Task::run()` drives the full lifecycle: spawn → Started → wait → Exited. Hosts the timeout monitor thread (M2).
- **os/mod.rs** — `OsProcess` trait + `OsHandle`. `cfg` gates select the platform impl.
- **os/windows.rs / unix.rs** — OS-specific spawn, wait, and kill escalation. Must not leak platform behavior into callers.

---

## Milestone constraints

Do not add any of these until the milestone that introduces them:

| Feature | Milestone |
|---|---|
| Process tree cleanup (Job Objects, setsid) | M3 |
| FFI boundary | M4 |
| Language bindings (.NET, Python) | M5/M6 |
| Async execution | M5/M6 |
| STDOUT/STDERR surfacing to callers | TBD |

---

## Error handling rules

- No `unwrap()` or `expect()` in library code (`src/`). Tests may use them.
- No swallowed errors — every `Result` must be propagated or explicitly mapped to `AerError`.
- No `Box<dyn Error>` — all errors are typed `AerError` variants.

---

## Testing conventions

- All tests live in `core/tests/integration_test.rs` (integration) or inline in `machine.rs` (state machine unit tests only).
- Integration tests must be platform-agnostic: use the `#[cfg(target_os = "windows")]` helper functions in the test file to select commands, never hardcode shell paths.
- Exit codes in tests: use 0–127 only (cross-platform safe range).

---

## Git conventions

- Conventional commits: `<type>(<scope>): Capitalized description`
- Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `ci`, `test`, `chore`
- No direct commits to `main`. All changes via PR.
- Close issues in the PR body (`Closes #n`), not in commit messages.
