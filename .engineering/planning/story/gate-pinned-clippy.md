---
format: aep.planning-md/3
id: story:gate-pinned-clippy
kind: story
status: implemented
title: The local gate lints on the pinned toolchain, and main's Gate is green
relations:
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: Taskfile.yml
- confidence: cited
  path: crates/llm-credentials/tests/adversary_secrets.rs
- confidence: cited
  path: crates/llm-credentials/tests/live_subscription_turn.rs
- confidence: cited
  path: crates/llm-docs/src/pages.rs
- confidence: cited
  path: crates/llm-http/tests/transport.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-07T02:25:05Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-07T02:25:05Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-07T02:53:48Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1,"verification":1},"asserted":{"test_result":1,"verification":1}}}
---
## Outcome

`main`'s Gate is green again, and the local `task rust` lints on the toolchain CI pins as well as
on current stable, so a lint that only the pinned clippy reports is caught before a push.

## Why

Gate run 37390477849 on `main` (`d8ab3691`, 2026-10-05T23:47Z) failed in job
`libraries-and-plan`: clippy 1.98.0 reported `clippy::manual_let_else` at
`crates/llm-http/tests/transport.rs:546` (`-D clippy::pedantic`), so "Rust libraries" exited 101
and "Conformance reports" found no `target/conformance` to upload. The commit was checked locally
on rustc 1.99.0 only, which does not report that lint. Clippy 1.99.0 in turn reports
`clippy::assert_is_empty` at `crates/llm-docs/src/pages.rs:156`,
`crates/llm-credentials/tests/live_subscription_turn.rs:159`, `:361`, `:458` and
`crates/llm-credentials/tests/adversary_secrets.rs:474`, `:486`, all added after
`story:lints-rust-1-99` closed.

## Acceptance

- `cargo +1.98.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` and the
  same on 1.99.0 exit 0.
- `Taskfile.yml` task `rust` runs clippy on the pinned 1.98.0 in addition to the default
  toolchain; the pin itself does not move.
- The Gate workflow is green on the pull request head that carries this change into `main`.

## ESS first

None: lint fixes in tests and a gate command; no behaviour, noun or contract changes.
