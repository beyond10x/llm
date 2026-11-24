---
format: aep.planning-md/3
id: story:openai-decisions-client
kind: story
status: implemented
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
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:58Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-11-24T21:59:03Z", actor: "human:operator", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
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

## Scope

Read by the coordinator from `git diff --stat` of the unit branch `impl/openai-decisions-client`
(head `5eb173744`) against its base `e2e90a962`, at the close of wave
2026-11-24-openai-decisions-client. Nine of the eleven `inferred` lines of the frontmatter held; two
were not touched.

| Path | Inferred | What the unit did |
| --- | --- | --- |
| `Cargo.toml` | yes | the workspace member `crates/llm-openai-decisions` |
| `crates/llm-openai-decisions/Cargo.toml` | yes | new package |
| `crates/llm-openai-decisions/src/lib.rs` | yes | new: `OpenAiDecisions`, `decide`, `decide_recorded`, target selection, the record and its price |
| `crates/llm-openai-decisions/tests/client.rs` | yes | new: the crate's one integration-test binary |
| `crates/llm-decide/src/classify.rs` | yes | `Path::DecisionEndpoint`, the port `DecisionEndpoint`, `EndpointClassifier`, `basis_points` |
| `checks/conformance/src/decisions.rs` | yes | the `Decide` command of the adapter |
| `contracts/decisions/scenarios` | yes | 6 scenarios |
| `spec/domains/decisions.yaml` | yes | command `llm.decisions.Decide` and its nouns |
| `docs/design.md` | yes | "Typed decisions", the decision endpoint part |
| `crates/llm-core/src/vocabulary.rs` | yes | not touched: the unit needed no new vocabulary |
| `spec/domains/catalog.yaml` | yes | not touched: the endpoint is reached through the turn route the request's `model` names |
| `crates/llm-openai-decisions/src/request.rs`, `response.rs`, `strict.rs` | no | new: the request and its bounds, the response reader, the strict JSON reader |
| `crates/llm-openai-decisions/tests/client/adversary_w28_wire.rs` | no | the adversary's 13 cases on the wire |
| `crates/llm-decide/src/lib.rs` | no | exports |
| `crates/llm-decide/tests/classify.rs`, `tests/classify/endpoint.rs`, `tests/classify/adversary_w28_endpoint.rs` | no | the endpoint path's cases as modules of the `classify` binary; the adversary's 4 cases |
| `checks/conformance/src/decisions/endpoint.rs`, `checks/conformance/Cargo.toml` | no | new: the real client against a loopback server |
| `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/schema/`, `contracts/baseline.json` | no | generated with ess 0.87.0; the floor is 1062 |
| `Cargo.lock` | no | the new package |
| `AGENTS.md`, `README.md`, `CHANGELOG.md`, `docs/implementation-status.md` | no | the crate's line, the conformance floor, the changelog, the status row |
| `website/data/status.json`, `website/docs/status.mdx`, `website/docs/reference/crates.md` | no | generated by `llm-docs generate` |

`llm-decide` keeps its dependencies: it holds the port, and `llm-openai-decisions` depends on it and
implements the port.
