---
format: aep.planning-md/3
id: story:offline-replay-eval
kind: story
status: implemented
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
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:59Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:59Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-24T23:13:37Z", actor: "human:operator", revision: 6, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
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

## Scope

Read by the coordinator from `git diff --stat` of the unit branch `impl/offline-replay-eval`
(head `42705fe19`) against its base `c71a525cb`, at the close of wave
2026-11-24-offline-replay-eval. All nine `inferred` lines of the frontmatter held.

| Path | Inferred | What the unit did |
| --- | --- | --- |
| `crates/llm-calls/src/replay.rs` | yes | new: the corpus document `llm.corpus/1`, its reader and bounds, `replay`, the stop at the cap and at an unknown cost, the request ids |
| `crates/llm-calls/src/lib.rs` | yes | exports |
| `crates/llm-calls/tests/replay.rs` | yes | new: the replay's cases and the adversary's (`adversary_w29_*`); a seventh integration-test binary of the crate |
| `crates/llm-cli/src/lib.rs` | yes | the command `llm replay` and its exit statuses |
| `crates/llm-cli/tests/command.rs` | yes | the command's cases and the adversary's |
| `checks/conformance/src/calls.rs` | yes | the `Replay` command of the adapter, over the real `replay` and `Routed` |
| `contracts/calls/scenarios` | yes | 6 scenarios |
| `spec/domains/calls.yaml` | yes | command `llm.calls.Replay`, entity `llm.calls.Corpus` and their nouns |
| `docs/design.md` | yes | "Call records", the replay part |
| `crates/llm-calls/src/shadow.rs`, `crates/llm-calls/Cargo.toml` | no | `pair_id` and `skipped` taken out of the shadow path for the replay to share; dependencies |
| `crates/llm-decide/src/replay.rs`, `src/lib.rs`, `Cargo.toml` | no | new: `Routed`, the candidate that sends a case to a turn, the classifier or the judge |
| `crates/llm-decide/tests/classify.rs`, `tests/classify/replay.rs` | no | `Routed`'s cases as a module of the `classify` binary |
| `crates/llm-cli/Cargo.toml` | no | dependencies |
| `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/schema/`, `contracts/baseline.json` | no | generated with ess 0.87.0; the floor is 1068 |
| `Cargo.lock` | no | dependency edges |
| `AGENTS.md`, `README.md`, `CHANGELOG.md`, `docs/implementation-status.md` | no | the conformance floor, the crate rows, the changelog, the status row |
| `website/docs/guides/use-the-llm-command.md`, `reference/formats.md`, `status/limitations.md` | no | the guide for `llm replay`, the corpus format, the limits |
| `website/data/status.json`, `website/docs/status.mdx`, `website/docs/reference/crates.md` | no | generated by `llm-docs generate` |

`replay` takes a `Candidate` port because `llm-decide` depends on `llm-calls`; `Routed` implements
it in `llm-decide`. The conformance target does not depend on `llm-cli`.
