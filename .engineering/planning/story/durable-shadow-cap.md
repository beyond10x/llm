---
format: aep.planning-md/3
id: story:durable-shadow-cap
kind: story
status: implemented
title: The shadow daily cap survives a restart and a second process
tags:
- from-finding
relations:
- informed_by: review-result:security-seams-u4-pass-1
- decomposes: epic:routing
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: checks/conformance/src/calls.rs
- confidence: inferred
  path: crates/llm-calls/Cargo.toml
- confidence: cited
  path: crates/llm-calls/src/shadow.rs
- confidence: inferred
  path: crates/llm-calls/tests/shadow.rs
revision: 20
transitions:
- {from: "draft", to: "proposed", at: "2026-11-10T00:19:25Z", actor: "human:timo", revision: 12}
- {from: "proposed", to: "active", at: "2026-11-10T00:19:25Z", actor: "human:timo", revision: 13}
- {from: "active", to: "implemented", at: "2026-11-10T04:21:42Z", actor: "human:timo", revision: 20, decided_on: {"recorded":{"test_result":1,"review_outcome":2}}}
---
## Context

`review-result:security-seams-u4-pass-1` found that the shadow daily cap
(`crates/llm-calls/src/shadow.rs`, `ShadowLedger`) lives in an in-memory budget engine, which
`crates/llm-cost/src/budget/engine.rs:9-10,41` documents as not a durable permission. A restart
during a UTC day, or a second process, starts each day's cap afresh. In the wave of 2026-11-05 the
cap is documented as per process; nothing in this repository runs a long-lived router on it.

## Outcome

A deployment can back each shadow's daily cap with the durable `SqliteLedger` (the
`spending-limits` capability), so a restart or a second process sharing the ledger file cannot
spend past the day's cap.

## Acceptance (draft)

- A shadow ledger opened on a durable ledger keeps a day's spend across a reopen; a scenario
  restarts mid-day and the next shadow past the cap is skipped.
- Two processes on one ledger file cannot both take the last reservation.

## Scope

Derived 2026-11-10 by `aep:story-scoper` at `c10908eb`; confidence high for `shadow.rs`, medium for the rest.

- cited: `crates/llm-calls/src/shadow.rs:43-149` (`ShadowLedger`, `Days`, `open`, `settle`); read only `crates/llm-cost/src/budget/sqlite.rs`, `crates/llm-cost/src/budget/engine.rs:41`.
- inferred: `crates/llm-calls/Cargo.toml` (a `sqlite` feature forwarding `llm-cost/sqlite`), `crates/llm-calls/tests/shadow.rs`, `checks/conformance/src/calls.rs:152-162`; documents that say "per process": `AGENTS.md:57`, `docs/design.md:641-645`, `docs/implementation-status.md:162`, `CHANGELOG.md`; generated `website/data/status.json` (regenerate).
- Smallest change (scoper): reuse `llm_cost::budget::SqliteLedger` behind an off-by-default `llm-calls` feature; a new constructor such as `ShadowLedger::durable(dir, …)` keeping one ledger per shadow and UTC day at `<dir>/<shadow-id>/<utc-day>`. Not breaking.
- Safety fact: `SqliteLedger` has one owner per directory; a second process's `open` is refused while the first holds `owner.lock` (`sqlite.rs:41-58`), and `ShadowLedger::open` turns any error into `Skipped` (`shadow.rs:350-354`), so a second process skips the shadow and never takes the reservation (unproven).
- Not established: day rollover layout; whether conformance needs a durable scenario.
- landed (wave 2026-11-09-github-zero, branch head `eaf0430d`, from `git diff --name-only`): `.github/workflows/gate.yml`, `Cargo.lock`, `Taskfile.yml`, `crates/llm-calls/Cargo.toml`, `crates/llm-calls/src/shadow.rs`, `crates/llm-calls/tests/adversary_w2026_11_09_u6.rs`, `crates/llm-calls/tests/shadow.rs`, `CHANGELOG.md`, `AGENTS.md`, `docs/design.md`, `docs/implementation-status.md`, `website/data/status.json` (coordinator).
