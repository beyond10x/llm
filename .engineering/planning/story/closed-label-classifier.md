---
format: aep.planning-md/3
id: story:closed-label-classifier
kind: story
status: active
title: A closed-label classifier returns one label with per-label confidence or an abstention with a cause
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:call-records
scope:
- confidence: inferred
  path: .engineering/specs/llm-seams/domains/decisions.yaml
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: checks/conformance/Cargo.toml
- confidence: inferred
  path: checks/conformance/src/decisions.rs
- confidence: inferred
  path: checks/conformance/src/target.rs
- confidence: inferred
  path: contracts/decisions/scenarios
- confidence: inferred
  path: crates/llm-decide/Cargo.toml
- confidence: inferred
  path: crates/llm-decide/src/chain.rs
- confidence: inferred
  path: crates/llm-decide/src/classify.rs
- confidence: inferred
  path: crates/llm-decide/src/lib.rs
- confidence: inferred
  path: crates/llm-decide/tests/classify.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/decisions.yaml
- confidence: inferred
  path: spec/system.yaml
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 4}
---
## Context

A downstream adopter needs a closed-label decision: one label of a fixed set, with a confidence
per label, or an abstention that says why, behind a registry with a fallback chain that can end
in a lexical classifier it supplies. Verified at llm 0.22.0: strict JSON-schema output
(`Controls::output_format`, `crates/llm-core/src/controls.rs:278-312`), forced tool choice
(`ToolChoice::Named`, `crates/llm-core/src/turn.rs:62-67`) and `llm-tool-call::call_tool` exist;
no label-set type, per-label confidence, abstention cause, chain or lexical classifier exists in
any beyond10x repository.

## Outcome

A new crate `llm-decide` holds the typed-decision family. Here: a `LabelSet` (versioned,
distinct labels, at least two); a `Classifier` trait returning either a `Decision` (one label of
the set and a confidence per label in basis points, unknown left absent) or an `Abstention` with
a cause `Timeout`, `ProviderError` (with the llm error code) or `OffLabel`; model classifiers on
two paths, `JsonSchema` (strict output schema built from the label set) and `ForcedTool` (one
forced tool whose arguments are the same schema); a registry that runs a chain in order until
one classifier decides and may end in a caller-supplied lexical classifier that makes no model
call. Every model call yields an `llm.calls.CallRecord`, which reports latency and cost per call.

## Specification

Domain `llm.decisions`, drafted at `.engineering/specs/llm-seams/domains/decisions.yaml`
(`LabelSet`, `Chain`, `Classifier`, `Decision`, `Abstention`, command `llm.decisions.Classify`;
`Rubric` and `Verdict` belong to `story:rubric-judge`). Validated with the other two draft domains
on 2026-11-05 (ess 0.86.0: `llm v1 — 18 file(s), valid`). This story moves the file to
`spec/domains/decisions.yaml` and adds `llm.decisions` to `spec/system.yaml`.

## Acceptance

Scenarios in `contracts/decisions/scenarios/`, listed in `contracts/ess-inputs.yaml`, answered:

- `a-json-schema-classifier-returns-one-label-with-a-confidence-per-label`
- `a-forced-tool-classifier-returns-one-label-with-a-confidence-per-label`
- `an-answer-naming-no-label-of-the-set-abstains-off-label`
- `an-answer-that-does-not-decode-as-the-schema-abstains-off-label`
- `a-classifier-past-the-chain-deadline-abstains-with-timeout`
- `a-provider-failure-abstains-with-its-error-code`
- `a-confidence-below-the-classifier-floor-abstains-off-label`
- `the-chain-falls-back-in-order-and-ends-in-the-caller-lexical-classifier`
- `a-chain-where-every-classifier-abstains-returns-the-last-abstention-as-final`
- `each-model-classifier-call-yields-a-call-record-with-latency-and-cost`
- `a-label-set-with-a-duplicate-or-a-single-label-is-refused`

And: `docs/design.md` gains a "Typed decisions" section (classification part); the conformance
baseline rises by the new scenarios and skips none.

## Not in scope

The provider decision endpoint (`story:openai-decisions-client`), the rubric judge
(`story:rubric-judge`), any domain-specific label set or prompt: the caller owns those.

## Verification

Targeted: `-p llm-decide --lib`, its named test targets, the conformance check. No live call.
