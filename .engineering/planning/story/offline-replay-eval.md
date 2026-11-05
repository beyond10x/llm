---
format: aep.planning-md/3
id: story:offline-replay-eval
kind: story
status: active
title: An offline replay sends a recorded corpus to a candidate route and compares, under a money cap
tags:
- from-adopter-report
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
- depends_on: story:call-records
- depends_on: story:closed-label-classifier
scope:
- confidence: inferred
  path: checks/conformance/src/calls.rs
- confidence: inferred
  path: contracts/calls/scenarios
- confidence: inferred
  path: crates/llm-calls/src/lib.rs
- confidence: inferred
  path: crates/llm-calls/src/replay.rs
- confidence: inferred
  path: crates/llm-calls/tests/replay.rs
- confidence: inferred
  path: crates/llm-cli/src/lib.rs
- confidence: inferred
  path: crates/llm-cli/tests/command.rs
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

A downstream adopter specified an offline replay eval as a router-level, provider-neutral
invariant: replay a recorded corpus against a candidate route and compare, under a money cap.
Verified at llm 0.22.0: llm has recorded-fixture replay in tests only; inference replays control
decisions and does not compare model output. No corpus format or model-output eval exists.

## Outcome

In `llm-calls`: a corpus document `llm.corpus/1` (cases, each a turn, a classification or a
verdict request and its recorded answer; the caller assembles it, llm records no text) and
`replay(corpus, candidate route, cap)`, which sends each case in order, compares the answer with
the recorded one (equal label, equal canonical verdict JSON, or equal text) and reports per case
`Same`, `Differs`, `Failed` or `NotSent`. Spend is held on an `llm.budget` ledger; a case whose
admission would pass the cap is not sent, and the run stops and counts the rest `NotSent`. Every
call's record has role `Replay`. The `llm` command gains `llm replay`, which runs it from files
and prints the summary as JSON.

## Specification

`llm.calls.ReplayRun` and command `llm.calls.Replay` in the draft
`.engineering/specs/llm-seams/domains/calls.yaml`, validated 2026-11-05 (ess 0.86.0).

## Acceptance

Scenarios in `contracts/calls/scenarios/`, listed in `contracts/ess-inputs.yaml`, answered:

- `a-replay-compares-each-case-with-its-recorded-answer`
- `a-replay-stops-at-its-money-cap-and-counts-the-cases-not-sent`
- `a-failed-candidate-call-counts-failed-not-differs`
- `two-replays-of-one-corpus-report-the-same-digest-and-counts`
- `replay-records-carry-role-replay`
- `a-classification-case-compares-labels-not-confidences`

And: `crates/llm-cli/tests/command.rs` gains `replay_prints_the_summary_and_exits_nonzero_past_the_cap`;
`docs/design.md` "Call records" gains the replay part; the baseline rises, skips none.

## Verification

Targeted: `-p llm-calls` named test targets, `-p llm-cli --test command`, the conformance check.
No live call.
