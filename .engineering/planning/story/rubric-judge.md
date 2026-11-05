---
format: aep.planning-md/3
id: story:rubric-judge
kind: story
status: active
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
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 4}
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
