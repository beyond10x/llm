---
format: aep.planning-md/3
id: story:openai-decisions-client
kind: story
status: active
title: The provider decision endpoint (POST /v1/decisions) as the classifier's third model path
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:closed-label-classifier
scope:
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: checks/conformance/src/decisions.rs
- confidence: inferred
  path: contracts/decisions/scenarios
- confidence: inferred
  path: crates/llm-core/src/vocabulary.rs
- confidence: inferred
  path: crates/llm-decide/src/classify.rs
- confidence: inferred
  path: crates/llm-openai-decisions/Cargo.toml
- confidence: inferred
  path: crates/llm-openai-decisions/src/lib.rs
- confidence: inferred
  path: crates/llm-openai-decisions/tests/client.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/catalog.yaml
- confidence: inferred
  path: spec/domains/decisions.yaml
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 6}
---
## Context

A downstream adopter reading llm 0.20.0 reported no client for the OpenAI Decisions API. Verified on
2026-11-04:

- The endpoint is in OpenAI's official reference: `POST /v1/decisions`, operation `createDecision`,
  tag `Decisions` (`openai/openai-openapi` `openapi.yaml`, commit `b4cad10d`, path `/decisions`;
  https://developers.openai.com/api/reference/resources/decisions). Request `DecisionRequest`:
  `model` (only `gpt-6-luna`), `input` (text, or message, function-call and custom-tool items, image
  parts allowed), `questions` (1 to 400, each a predicate, choice or score), `prompt_cache_key`,
  `prompt_cache_options`, `safety_identifier`; `additionalProperties: false`. Response
  `DecisionResponse`: `answers` in question order (predicate, choice, score or refusal, each with
  probabilities), `model`, `usage`. Non-streaming.
- Nothing in llm calls it: no crate, protocol, spec domain or document names the endpoint.

## Taken in (2026-11-05)

Scoped as the third model path of the closed-label classifier (`story:closed-label-classifier`):
`Path::DecisionEndpoint`. The open questions of the draft are settled so: it is not a turn, so it
is a separate client behind the classifier, not a `Model::turn`; its nouns (question kinds
`Predicate`, `Choice`, `Score`; answer kinds plus `Refusal`) are types of the `llm.decisions`
domain (`.engineering/specs/llm-seams/domains/decisions.yaml`); `docs/design.md` "Typed
decisions" records the design. A classifier asks one `Choice` question whose choices are the label
set; the answer's probabilities become per-label confidences in basis points.

## Outcome

A caller asks a Decisions-capable binding a set of typed questions about an input and gets typed
answers with probabilities and usage, through llm's credentials, transport and accounting; a
classifier on the `DecisionEndpoint` path returns a `Decision` or an `Abstention` like the other
two paths.

## Acceptance

Scenarios in `contracts/decisions/scenarios/`, listed in `contracts/ess-inputs.yaml`, answered:

- `a-choice-answer-becomes-a-decision-with-a-confidence-per-label`
- `a-refusal-answer-abstains-off-label`
- `an-unknown-answer-type-is-refused-by-its-path`
- `a-request-field-outside-the-reference-is-refused-by-name`
- `an-unreported-usage-counter-stays-unknown`
- `a-decision-endpoint-call-yields-a-call-record-priced-from-the-cost-table`

And: the client sends `POST /v1/decisions` over `llm-http` with an injected credential; fixture
bodies come from the reference's examples; no live call in the gate.

## Verification

Targeted: the client crate's named test targets and the conformance check. No live call.
