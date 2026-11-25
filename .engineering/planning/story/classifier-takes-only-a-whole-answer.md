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
revision: 6
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

## Scope

Derived 2026-11-25 by `story-scoper` at `87bc16f16`. Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-decide` — cited
- **Files:** `crates/llm-decide/src/classify.rs:770-790` — cited; `ModelClassifier::answer`: the JSON-schema arm (779-787) becomes a call to `answer::whole`, the forced-tool arm (771-777) gains a stop-reason check
- **Files:** `crates/llm-decide/src/answer.rs:28-46` — cited; `whole` is the judge's reader (`judge.rs:312`), `pub(crate)`, and becomes the shared one; doc comment at most
- **Files:** `crates/llm-decide/tests/classify.rs` — inferred; the fake's `Reply::Text` (171-178) fixes `EndTurn` and one item, so it needs a reply with a stop reason and several items; the new tests land here or as a module under `tests/classify/`
- **Files:** `checks/conformance/src/decisions.rs` — inferred; `Script` (301-325) and `Scripted::answer` (370-411) fix `EndTurn` and one text item, so a scenario cannot script a cut or split answer without new members
- **Files:** `spec/domains/decisions.yaml` — inferred; prose only (header 9-13, `OffLabel` comment 125-126, `Classify` program comment 430-432), no field change
- **Files:** `contracts/decisions/scenarios/` — inferred; new scenarios beside `an-answer-that-does-not-decode-as-the-schema-abstains-off-label.yaml`
- **Files:** `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json` — inferred; scenario list near 997-1001, regenerated suite, floor 1068 rises
- **Symbols:** `ModelClassifier::answer`, `answer::whole`, `LabelSet::decode`, `settle`, `StopReason` — cited
- **Also likely:** `crates/llm-decide/src/replay.rs:115` — inferred; not edited, but `Routed::label` uses the JSON-schema classifier, so a cut or split recorded answer now reads `OffLabel`
- **Documents:** `docs/design.md` section "Typed decisions" (811; classifier paragraph 824-832; whole-answer list 900-916 names only the verdict) — cited
- **Documents:** `CHANGELOG.md` Unreleased, `AGENTS.md:80` (floor row) — inferred
- **Documents:** `docs/implementation-status.md:159` and the generated `website/data/status.json`, `website/docs/status.mdx` — inferred; only if the row is reworded. No hand-written website page states the classifier's reading
- **Confidence:** high for the JSON-schema path (the story names the site, the tree holds the reader); medium for the forced-tool path
- **Would collide with:** any unit touching `crates/llm-decide/src/classify.rs`, `tests/classify.rs`, the `llm.decisions` specification, adapter or scenarios, and every unit that adds a scenario (`contracts/ess-inputs.yaml`, `suite.json`, `baseline.json`, `AGENTS.md:80`, `CHANGELOG.md` Unreleased) — cited
- **Safety fact:** `whole` returns `None` for a superset of what the current arm rejects and `settle` turns `None` into `OffLabel` (`classify.rs:530-554`), so no answer that abstains today decides after the change; the reverse risk is a decoder that emits two `AssistantText` items for one answer — unproven — inferred
