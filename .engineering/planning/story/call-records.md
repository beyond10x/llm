---
format: aep.planning-md/3
id: story:call-records
kind: story
status: implemented
title: Every routed model call yields a call record with cost and no text
tags:
- from-adopter-report
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: .engineering/specs/llm-seams/domains/calls.yaml
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: checks/conformance/Cargo.toml
- confidence: inferred
  path: checks/conformance/src/calls.rs
- confidence: inferred
  path: checks/conformance/src/target.rs
- confidence: inferred
  path: contracts/calls/scenarios
- confidence: inferred
  path: crates/llm-calls/Cargo.toml
- confidence: inferred
  path: crates/llm-calls/src/lib.rs
- confidence: inferred
  path: crates/llm-calls/src/record.rs
- confidence: inferred
  path: crates/llm-calls/tests/record.rs
- confidence: inferred
  path: crates/llm-routing/src/fallback.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/calls.yaml
- confidence: inferred
  path: spec/system.yaml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:57Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-06T10:56:56Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

A downstream adopter needs a per-call record of every model call it makes through llm: which
route, target and model answered, in what role, how long it took, the usage the provider
reported and the priced cost, and no prompt or answer text. Verified at llm 0.22.0: `Usage`
(`crates/llm-core/src/turn.rs:298-317`), `llm-cost` (`PriceBook`, `AttemptUsage`, `CostReport`)
and per-attempt records in `llm-routing` (`Attempt`, `AttemptResult`) exist, but no record joins
them for a caller of a routed `Model`. The deployed ledger stays with llm-gateway
(`story:usage-records` there); this story is the client-side record and its sink only.

## Outcome

Every model call a routed caller makes yields one `CallRecord` on a sink the caller injects:
route, target, upstream model, role (`Primary`, `Shadow`, `Replay`), kind (`Turn`,
`Classification`, `Verdict`, `EmbedBatch`), outcome, error code, attempt count, start time,
latency, time to first event, usage with unknown left unknown, and cost from a caller-supplied
price book with its revision, marked incomplete when a line is unknown. The record type has no
text field.

## Specification

Domain `llm.calls`, drafted at `.engineering/specs/llm-seams/domains/calls.yaml` (entities
`CallRecord`, `Shadow`, `ReplayRun`; command `llm.calls.Call`). Validated on 2026-11-05 with ess
0.86.0 in a copy of `spec/` with the three draft domains added: `llm v1 — 18 file(s), valid`.
This story moves the file to `spec/domains/calls.yaml` and adds `llm.calls` to
`spec/system.yaml`.

## Acceptance

Scenarios in `contracts/calls/scenarios/`, listed in `contracts/ess-inputs.yaml`, each answered:

- `a-routed-turn-writes-one-record-with-no-text`
- `a-fallback-run-records-the-target-that-answered-and-its-attempt-count`
- `a-record-keeps-unknown-usage-unknown-and-its-cost-incomplete`
- `a-failed-call-records-its-error-code-and-dispatch-without-a-zero-cost`
- `a-record-carries-latency-and-time-to-first-event-from-the-scripted-clock`
- `a-call-without-a-price-book-records-usage-and-no-cost`

And:

- `crates/llm-calls/tests/record.rs::a_serialized_record_holds_none_of_the_turns_text`: a turn
  with distinctive prompt, answer and tool-argument strings yields a record whose JSON contains
  none of them.
- `docs/design.md` gains a "Call records" section; the conformance baseline rises by the new
  scenarios and skips none.

## Verification

Targeted: `-p llm-calls --test record`, the conformance check. No live call.
