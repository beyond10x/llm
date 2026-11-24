---
format: aep.planning-md/3
id: story:classifier-takes-only-a-whole-answer
kind: story
status: active
title: A JSON-schema classifier decides only on a whole, unambiguous answer
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w27-rubric-judge-pass-1
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 3}
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
