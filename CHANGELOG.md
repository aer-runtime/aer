# Changelog

## [0.4.0](https://github.com/aer-runtime/aer/compare/core-v0.3.0...core-v0.4.0) (2026-06-28)


### Features

* **core:** M4 FFI boundary — C-compatible ABI ([#43](https://github.com/aer-runtime/aer/issues/43)) ([d44ad57](https://github.com/aer-runtime/aer/commit/d44ad57380af1aa50de3dfa658a82a1a005bd936))
* **core:** M4b — Observation Tier (StdoutChunk/StderrChunk capture) ([#47](https://github.com/aer-runtime/aer/issues/47)) ([259060f](https://github.com/aer-runtime/aer/commit/259060f7635a27ae9ebd2b79e51fe61dc767c473)), closes [#44](https://github.com/aer-runtime/aer/issues/44)
* **core:** M4c — Cancellation and ExitReason ([#48](https://github.com/aer-runtime/aer/issues/48)) ([8d32071](https://github.com/aer-runtime/aer/commit/8d3207173593a9ff5671576de1fc56dc93e626a6)), closes [#45](https://github.com/aer-runtime/aer/issues/45)


### Documentation

* **examples:** Add capture and cancel examples for M4b/M4c ([#50](https://github.com/aer-runtime/aer/issues/50)) ([e8af587](https://github.com/aer-runtime/aer/commit/e8af587f91c854a8361ac8a9a40e274b5849411c)), closes [#49](https://github.com/aer-runtime/aer/issues/49)

## [0.3.0](https://github.com/aer-runtime/aer/compare/core-v0.2.0...core-v0.3.0) (2026-06-26)


### Features

* **core:** Process tree cleanup — Windows Job Objects + Unix setsid ([#36](https://github.com/aer-runtime/aer/issues/36)) ([c9792a4](https://github.com/aer-runtime/aer/commit/c9792a44f7e4295aabbbda6a508630875cc2c5bd))


### Bug Fixes

* **core:** Concurrent pipe drain, Unix orphan cleanup on wait error, M3 docs ([#40](https://github.com/aer-runtime/aer/issues/40)) ([cab0d44](https://github.com/aer-runtime/aer/commit/cab0d441db46d30da956b6576ba4f4c7878636b5))
* **core:** Process tree deadlock fix, pre_exec safety, and M3 tests ([#39](https://github.com/aer-runtime/aer/issues/39)) ([a64c682](https://github.com/aer-runtime/aer/commit/a64c682a10c0b956b5b0f99ab7c0a75ddbb1709d))


### Documentation

* **examples:** Add M2 timeout and M3 process tree examples ([#41](https://github.com/aer-runtime/aer/issues/41)) ([3d2a116](https://github.com/aer-runtime/aer/commit/3d2a116d54c2c83f9aa3a54d279695b1961a8991))

## [0.2.0](https://github.com/aer-runtime/aer/compare/core-v0.1.1...core-v0.2.0) (2026-06-26)


### Features

* **core:** Timeout and kill escalation (M2) ([#27](https://github.com/aer-runtime/aer/issues/27)) ([a97e623](https://github.com/aer-runtime/aer/commit/a97e623c96bfaedf3ab8a3fb60be2122c6305602))


### Tests

* **core:** M2 timeout and kill escalation integration tests ([#28](https://github.com/aer-runtime/aer/issues/28)) ([c063674](https://github.com/aer-runtime/aer/commit/c06367466d9a4f9d90baa793eaf7de3ca48a809d))

## [0.1.1](https://github.com/aer-runtime/aer/compare/core-v0.1.0...core-v0.1.1) (2026-06-26)


### Miscellaneous

* **core:** Milestone 1 — core scaffold and process lifecycle ([#14](https://github.com/aer-runtime/aer/issues/14)) ([abf11ff](https://github.com/aer-runtime/aer/commit/abf11ff32bede0adfd64cf8cef9952cd5ca2caae))
