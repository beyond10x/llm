---
format: aep.planning-md/3
id: story:shadow-calls
kind: story
status: active
title: A shadow route answers beside the primary, unseen by the caller, under a daily money cap
tags:
- from-adopter-report
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
- depends_on: story:call-records
scope:
- confidence: inferred
  path: checks/conformance/src/calls.rs
- confidence: inferred
  path: contracts/calls/scenarios
- confidence: inferred
  path: crates/llm-calls/src/lib.rs
- confidence: inferred
  path: crates/llm-calls/src/shadow.rs
- confidence: inferred
  path: crates/llm-calls/tests/shadow.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/calls.yaml
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:59Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:59Z", actor: "human:timo", revision: 4}
---
## Context

A downstream adopter specified shadow calls as a router-level, provider-neutral invariant: a
second route answers the same request beside the primary, out of the caller's sight, under a
daily money cap. Verified at llm 0.22.0: `llm-routing` has ordered fallback and `llm-cost` a
budget cap engine (`llm-cost` `budget`, `llm.budget.Ledger`); there is no route role, detached
call or daily cap (grep for shadow, role, daily cap in `crates/`, `spec/`).

## Outcome

In `llm-calls`: a `Shadow` binds a primary route to a shadow route with a daily cap in a ledger's
currency. After the primary call has returned to the caller, the same request goes to the shadow
route as a detached call. Its answer reaches only its `CallRecord` (role `Shadow`, naming the
primary's record) and never the caller's sink. A shadow call is admitted on an `llm.budget`
ledger keyed by the UTC day; past the cap it is not sent and is recorded `Skipped`. A shadow
failure, slowness or skip never changes, delays or fails the primary.

## Specification

`llm.calls.Shadow` and command `llm.calls.Call` (input `shadows_json`) in the draft
`.engineering/specs/llm-seams/domains/calls.yaml`, validated 2026-11-05 (ess 0.86.0).
`story:call-records` moves the domain into `spec/`; this story adds its scenarios.

## Acceptance

Scenarios in `contracts/calls/scenarios/`, listed in `contracts/ess-inputs.yaml`, answered:

- `a-shadow-answer-never-reaches-the-caller`
- `a-shadow-call-starts-only-after-the-primary-returned`
- `a-shadow-past-its-daily-cap-is-skipped-and-recorded`
- `the-daily-cap-starts-again-at-utc-midnight`
- `a-failing-shadow-leaves-the-primary-result-unchanged`
- `a-shadow-record-names-its-primary-record`

And: `docs/design.md` "Call records" gains the shadow part; the baseline rises, skips none.

## Verification

Targeted: `-p llm-calls` named test targets and the conformance check. No live call.
