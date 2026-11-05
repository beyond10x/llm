---
format: aep.planning-md/3
id: story:structured-output
kind: story
status: implemented
title: A turn asks for JSON-schema structured output on every wire
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-bedrock/src/converse.rs
- confidence: inferred
  path: crates/llm-chat/src/ingress.rs
- confidence: inferred
  path: crates/llm-chat/src/outgoing.rs
- confidence: inferred
  path: crates/llm-core/src/turn.rs
- confidence: inferred
  path: crates/llm-core/src/vocabulary.rs
- confidence: inferred
  path: crates/llm-messages/src/codec.rs
- confidence: inferred
  path: crates/llm-messages/src/ingress.rs
- confidence: inferred
  path: crates/llm-responses/src/ingress.rs
- confidence: inferred
  path: crates/llm-responses/src/request.rs
- confidence: inferred
  path: crates/llm-routing/src/selection.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/bedrock.yaml
- confidence: inferred
  path: spec/domains/chat.yaml
- confidence: inferred
  path: spec/domains/inference.yaml
- confidence: inferred
  path: spec/domains/messages.yaml
- confidence: inferred
  path: spec/domains/responses.yaml
revision: 22
transitions:
- {from: "draft", to: "proposed", at: "2026-11-04T07:35:27Z", actor: "human:timo", revision: 18}
- {from: "proposed", to: "active", at: "2026-11-04T07:35:27Z", actor: "human:timo", revision: 19}
- {from: "active", to: "implemented", at: "2026-11-05T01:13:10Z", actor: "human:timo", revision: 22, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

A downstream adopter reading llm 0.20.0 reported that a turn cannot ask for JSON-schema structured
output; today it forces a tool call to get typed output. Verified against the code on 2026-11-04:

- `TurnRequest` (`crates/llm-core/src/turn.rs:77`) and `Controls` (`crates/llm-core/src/controls.rs:262`)
  have no output-format field.
- Responses ingress reads `text` with only `verbosity`; "Structured output (`text.format`) is outside
  this design" (`crates/llm-responses/src/request.rs:474`).
- Chat ingress refuses `response_format` by name (`crates/llm-chat/src/ingress.rs:55`).
- Messages reads `output_config` with only `effort` (`crates/llm-messages/src/codec.rs:698`) and writes
  only `effort` (`crates/llm-messages/src/codec.rs:236`).
- `docs/design.md:613` lists structured output (`output_config.format`, `text.format`,
  `response_format`) as refused and outside the design.

Every wire llm speaks has a documented field for it:

- OpenAI Responses `text.format` `{type: json_schema, name, schema, strict}` and Chat Completions
  `response_format` `{type: json_schema, json_schema: {name, schema, strict}}`
  (`openai/openai-openapi` `openapi.yaml`, schema `TextResponseFormatJsonSchema`).
- Anthropic Messages `output_config.format` `{type: json_schema, schema}`, no beta header; the
  deprecated `output_format` needs beta `structured-outputs-2025-11-13`
  (https://platform.claude.com/docs/en/build-with-claude/structured-outputs).
- Bedrock Converse `outputConfig.textFormat` `{type, structure}`
  (https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html); Anthropic
  InvokeModel takes the Messages body.

## Outcome

A caller asks for output that conforms to a JSON Schema, and every wire carries the request or
refuses it before I/O; none drops it.

## Acceptance

- `Controls` carries an optional output format (`Option`, absent by default): a name, a JSON Schema
  and a strict flag. Absent is the 0.20.0 turn exactly, byte for byte in `TurnDocument`. Additive
  only (approval-record:wave-2026-11-04-w25-proposal): no field on `TurnRequest` or `Capabilities`,
  no new variant on a public enum.
- Responses sends it as `text.format`, Chat as `response_format`, Messages and Anthropic InvokeModel
  as `output_config.format`, Bedrock Converse as `outputConfig.textFormat`; each is a recorded fixture
  body.
- The Responses, Chat and Messages ingress decoders read the same fields into the neutral format
  instead of refusing them, and the ingress encoders round-trip them; an unknown key inside them is
  still refused by its path.
- `strict` on Messages, Anthropic InvokeModel and Converse: those wires always constrain the answer
  to the schema (Anthropic structured outputs; Bedrock "strict schema compliance" through constrained
  decoding, https://docs.aws.amazon.com/bedrock/latest/userguide/structured-output.html), so
  `strict: true` is honoured by construction and sends no member, and `strict: false` is refused by
  name before I/O. Amended after adversary pass 1 (`review-result:adversary-w25-structured-output-pass-1`).
- `name` is sent wherever the wire has a place for it, Converse `structure.jsonSchema.name`
  included; Messages has none, and `docs/design.md` "Client request fields" says so. Nothing is
  dropped silently, and no new `Advisory` variant.
- The assistant text is returned as text; llm does not validate it against the schema and says so in
  `docs/design.md`.
- ESS: the `llm.inference` domain and the four protocol domains declare the field; authored
  scenarios per wire; the conformance baseline rises and skips none.

## Verification

Targeted tests per touched crate, fixture bodies per wire, conformance scenarios. No live call.
