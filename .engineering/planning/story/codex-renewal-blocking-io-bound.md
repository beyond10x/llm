---
format: aep.planning-md/3
id: story:codex-renewal-blocking-io-bound
kind: story
status: active
title: A stalled Codex renewal file read ends within a bound
summary: The renewal's blocking file I/O is unbounded, so the runtime cannot shut down while it hangs
tags:
- from-finding
relations:
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
- informed_by: review-result:adversary-w10-codex-test-pass-2
scope:
- confidence: inferred
  path: crates/llm-credentials/src/codex.rs
- confidence: cited
  path: crates/llm-credentials/src/codex/renewal.rs
- confidence: cited
  path: crates/llm-credentials/src/local.rs
- confidence: cited
  path: crates/llm-credentials/tests/adversary_codex_renewal.rs
- confidence: inferred
  path: docs/local-secrets.md
revision: 12
transitions:
- {from: "draft", to: "proposed", at: "2026-11-10T00:19:25Z", actor: "human:timo", revision: 10}
- {from: "proposed", to: "active", at: "2026-11-10T00:19:25Z", actor: "human:timo", revision: 11}
---
## Context

`review-result:adversary-w10-codex-test-pass-2`, finding at `crates/llm-credentials/tests/adversary_codex_renewal.rs:104` (pre-existing, CONFIRMED): a stall in the Codex renewal's blocking file I/O is unbounded, so the runtime cannot shut down while it hangs. `crates/llm-credentials/src/local.rs:70` runs it on an unbounded `tokio::task::spawn_blocking`. Not traced end to end by the triage of 2026-11-10; the implementor confirms it with a failing test first or reports it does not hold.

## Outcome

A renewal whose file I/O stalls ends with a typed, retriable-or-final error within a stated bound, and dropping the runtime does not wait on it.

## Acceptance

Amended 2026-11-10 by the coordinator from the scoper: a FIFO already at the path cannot stall the renewal, so the first draft's test would pass unchanged. The stall that remains is the write-back re-read.

The write-back re-read (`renewal.rs:844`) opens the login file as the first read does (no-follow, `NONBLOCK`, `fstat` regular-file check), so a path that is a FIFO at re-read time is refused by a typed error and never blocks; a test drives that re-read on a FIFO in a `tempfile` directory and ends within one second.

## Scope

Derived 2026-11-10 by `aep:story-scoper` at `c10908eb`; confidence high.

- cited: `crates/llm-credentials/src/codex/renewal.rs:844` (the write-back re-read is a plain `fs::read` after the `lstat` at `:836-837`, so a path swapped for a FIFO between them blocks), `crates/llm-credentials/src/local.rs:62-78` (`blocking` has no limit; a cancelled waiter keeps its permit), `crates/llm-credentials/tests/adversary_codex_renewal.rs`.
- cited: a FIFO already at the path is refused before any blocking read (`renewal.rs:644`, `codex.rs:267` `NONBLOCK`, `renewal.rs:649` `fstat`). `resolve_now` holds the `bound` mutex across both blocking awaits (`renewal.rs:571-595`), so one stall blocks every later resolve; eight stalls use up the shared permits (`local.rs:39`).
- inferred: `crates/llm-credentials/src/codex.rs:258-274` (reuse its `NONBLOCK` open and `fstat` check at `:844`), `docs/local-secrets.md:133-134,275-276`, `CHANGELOG.md`.
