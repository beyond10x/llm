---
format: aep.planning-md/3
id: story:classifier-takes-only-a-whole-answer
kind: story
status: implemented
title: A JSON-schema classifier decides only on a whole, unambiguous answer
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w27-rubric-judge-pass-1
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: checks/conformance/src/decisions.rs
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/decisions/scenarios
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-decide/src/answer.rs
- confidence: cited
  path: crates/llm-decide/src/classify.rs
- confidence: inferred
  path: crates/llm-decide/tests/classify.rs
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: spec/domains/decisions.yaml
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T03:17:38Z", actor: "human:operator", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

The adversary's first pass on `story:rubric-judge` (`review-result:adversary-w27-rubric-judge-pass-1`)
found that the judge took an answer as a verdict when it repeated a key, was cut at the output limit
or came as two text items. The judge was corrected in commit `bf58d42a`. The implementor of that
correction reported the same class in the classifier's JSON-schema path, which that unit did not
touch: `crates/llm-decide/src/classify.rs:650-657` joins text items, does not read `stop_reason` and
keeps the last value of a repeated key. The coordinator did not re-read those lines. The classifier
shipped in 0.23.0 (`story:closed-label-classifier`), so the defect was there before the wave.

## Outcome

A classifier on the JSON-schema path takes an answer as a decision only when it is the one text item
of a turn that ended normally and is one JSON value with no repeated key; otherwise it abstains
`OffLabel`, as it does for an answer that does not decode as the schema. The forced-tool path is read
for the same class and either holds it or is corrected.

## Acceptance

- An answer that repeats `label`, an answer cut at the output limit, and an answer in two text items
  each abstain `OffLabel` on the JSON-schema path; a test per case fails before the change.
- The forced-tool path has a test per case of the class that can reach it.
- One reader of an answer serves the classifier and the judge.

## Verification

Targeted: `-p llm-decide` named test targets and the conformance check. No live call.

## Scope

Read by the coordinator from `git diff --stat` of the unit branch `impl/classifier-whole-answer` (head
`2106be960`) against its base `0fa28f936`, at the close of wave 2026-11-25-classifier-whole-answer.
The `story-scoper`'s section of 2026-11-25 (read at `87bc16f16`) named the crate, `classify.rs` and
`answer::whole`; every `inferred` line of the frontmatter held except
`docs/implementation-status.md`, which the unit did not touch.

| Path | Scoped | What the unit did |
| --- | --- | --- |
| `crates/llm-decide/src/classify.rs` | cited | `ModelClassifier::answer` is two calls: `answer::call` on the forced-tool path, `answer::whole` on the JSON-schema path; doc comments |
| `crates/llm-decide/src/answer.rs` | cited, "doc comment at most" | also the new `pub(crate) fn call` and two unit tests for what only a direct reading reaches (the tool name, a turn that says it ended normally) |
| `docs/design.md` | cited | "Typed decisions": the classifier's reading on both paths, the empty text item beside an answer |
| `crates/llm-decide/tests/classify.rs` | inferred | the fake's `Reply::Outcome(StopReason, Vec<Item>)`; the cases are in the new module `tests/classify/whole_answer.rs` (18 tests), one more in `tests/classify/replay.rs` |
| `checks/conformance/src/decisions.rs` | inferred | the script gained `answers`, `tool_calls` and `stop_reason`; `tool_arguments` and `tool_calls` are read as JSON where the program is read; one unit test |
| `spec/domains/decisions.yaml` | inferred | prose only: the header, the `OffLabel` comment, the `Classify` program comment |
| `contracts/decisions/scenarios` | inferred | 5 scenarios |
| `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json` | inferred | generated with ess 0.87.0; the floor is 1080 (was 1075, not the 1068 the scoper read before wave 4) |
| `CHANGELOG.md`, `AGENTS.md` | inferred | six lines under `### Fixed`, the first marked breaking; the floor row |
| `docs/implementation-status.md` | inferred | not touched: its row stays true |

Not touched, as scoped: `crates/llm-decide/src/replay.rs` (its result for a cut or split classification
changed through the classifier and is held by a test), `crates/llm-decide/src/judge.rs`, every protocol
crate, `crates/llm-core/`, `crates/llm-openai-decisions/`. `contracts/schema/` regenerated to the same
bytes. Not covered: a key given twice inside tool arguments
(`story:tool-arguments-with-a-repeated-key`).
