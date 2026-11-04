---
format: aep.planning-md/3
id: story:openai-decisions-client
kind: story
status: draft
title: A client for the OpenAI Decisions API (POST /v1/decisions)
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 1
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

## Outcome

A caller asks a Decisions-capable binding a set of typed questions about an input and gets typed
answers with probabilities and usage, through llm's credentials, transport and accounting.

## Open before acceptance

- It is not a turn: `Model::turn` and the neutral `TurnRequest` do not fit a question set and
  probability answers. It needs a second port or a separate client, a new protocol value and new
  nouns (question, answer), so an ESS domain first and a design decision in `docs/design.md`.
- The API is new and names one model; its error classes and retry rules are not documented beyond
  the schema.

## Acceptance (draft)

- An ESS domain declares the decision request, question kinds, answer kinds and failures.
- A client sends `POST /v1/decisions` over `llm-http` with an injected credential and decodes the
  answers; every request field is carried or refused by name, and an unknown answer type is refused.
- Usage is recorded with unknown counters kept unknown; pricing comes from the cost table.
- Fixture bodies taken from the reference's examples; no live call in the gate.
