---
format: aep.planning-md/3
id: story:rubric-judge
kind: story
status: implemented
title: A rubric judge returns a strict JSON verdict with model and rubric version pinned
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:closed-label-classifier
scope:
- confidence: inferred
  path: checks/conformance/src/decisions.rs
- confidence: inferred
  path: contracts/decisions/scenarios
- confidence: inferred
  path: crates/llm-decide/src/judge.rs
- confidence: inferred
  path: crates/llm-decide/src/lib.rs
- confidence: inferred
  path: crates/llm-decide/tests/judge.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/decisions.yaml
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-24T15:49:34Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

A downstream adopter needs an eval judge as a service: one rubric call that returns a strict JSON
verdict, with the model pinned. Its scenarios, personas and faithfulness checks stay with the
adopter. Verified at llm 0.22.0: no beyond10x repository holds a model-backed judge (grep of
metaharness, bench, harness, llm, loom for judge and rubric); the pieces exist in llm (strict
JSON-schema output, catalog route pinning).

## Outcome

In `llm-decide`, beside the classifier: a `Rubric` (id, version, text, verdict JSON Schema, a
route) and `judge(rubric, subject)`, which makes one call with the rubric's schema as strict
output and returns a `Verdict` naming the rubric version, the rubric digest (sha256 of text and
schema), the route target and the upstream model that answered, and the verdict as canonical JSON.
An answer that does not decode as the schema is refused, never repaired. Stable across runs: the
request bytes are a function of rubric and subject only, sampling is fixed where the target
declares temperature, and the route must have exactly one target with fallback off, so the model
cannot change between runs.

## Specification

`llm.decisions.Rubric` and `llm.decisions.Verdict`, command `llm.decisions.Judge`, in the draft
`.engineering/specs/llm-seams/domains/decisions.yaml` (validated 2026-11-05, ess 0.86.0).
`story:closed-label-classifier` moves the domain into `spec/`; this story adds its scenarios.

## Acceptance

Scenarios in `contracts/decisions/scenarios/`, listed in `contracts/ess-inputs.yaml`, answered:

- `a-verdict-names-the-rubric-version-digest-and-upstream-model`
- `an-answer-not-valid-against-the-verdict-schema-is-refused-and-not-repaired`
- `the-same-answer-gives-byte-identical-verdict-json-on-two-runs`
- `two-judge-runs-over-one-rubric-and-subject-send-byte-identical-requests`
- `a-rubric-route-with-more-than-one-target-or-fallback-is-refused`
- `a-verdict-call-yields-a-call-record-of-kind-verdict`

And: `docs/design.md` "Typed decisions" gains the verdict part; the baseline rises, skips none.

## Verification

Targeted: `-p llm-decide` named test targets and the conformance check. No live call.

## Scope

Read by the coordinator from `git diff --stat` of the unit branch `impl/rubric-judge` (head
`20f06fa6c`) against its base, at the close of wave 2026-11-24-rubric-judge. The seven `inferred`
lines of the frontmatter all held. Lines the scoping did not have:

| Path | Inferred | What the unit did |
| --- | --- | --- |
| `crates/llm-decide/src/judge.rs` | yes | new: `Rubric`, `Verdict`, `judge`, `RubricError`, `JudgeError`, canonical JSON |
| `crates/llm-decide/src/lib.rs` | yes | exports |
| `crates/llm-decide/tests/judge.rs` | yes | new |
| `checks/conformance/src/decisions.rs` | yes | the judge adapter |
| `contracts/decisions/scenarios` | yes | 6 scenarios |
| `spec/domains/decisions.yaml` | yes | 5 fields on `VerdictObservation` and `LastVerdict` |
| `docs/design.md` | yes | "Typed decisions", the verdict part |
| `crates/llm-decide/src/answer.rs` | no | new, private: the strict reader of a whole answer (correction round 1) |
| `crates/llm-decide/src/schema.rs` | no | new, private: the schema subset and exact number comparison |
| `crates/llm-decide/src/classify.rs` | no | 4 lines |
| `crates/llm-decide/Cargo.toml`, `Cargo.lock` | no | `sha2`; `serde_json` features `float_roundtrip` and `unbounded_depth` |
| `crates/llm-decide/tests/adversary_w27_judge.rs`, `adversary_w27_judge_pass2.rs` | no | the adversary's 12 cases |
| `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/schema/`, `contracts/baseline.json` | no | generated with ess 0.87.0; the floor is 1055 |
| `AGENTS.md`, `README.md`, `CHANGELOG.md`, `docs/implementation-status.md` | no | the crate's line, the status line, the changelog |
| `website/data/status.json`, `website/docs/status.mdx`, `website/docs/reference/crates.md` | no | generated by `task docs-generate` |

`serde_json` feature `float_roundtrip` is unified across the workspace build: every crate that
reads a JSON number in the same build reads it with that feature on.
