---
format: aep.planning-md/3
id: story:responses-ingress-encoder
kind: story
status: active
title: A server answers a Responses caller from a neutral turn
summary: StreamEvents and TurnOutcome encode to the Responses SSE sequence and response object the repo's decoder accepts
relations:
- decomposes: epic:routes-and-ingress
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/responses.rs
- confidence: cited
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/responses/scenarios
- confidence: cited
  path: crates/llm-responses/src
- confidence: cited
  path: crates/llm-responses/tests
- confidence: cited
  path: spec/domains/responses.yaml
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "proposed", to: "active", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 10, decided_on: {"recorded":{"review_outcome":2}}}
---
## Context

A server answering a Responses caller can already read the caller's request onto the neutral turn
(`llm_responses::ingest_request`, `crates/llm-responses/src/request.rs`), but it cannot answer: nothing
turns `StreamEvent`s and a `TurnOutcome` back into the Responses server-sent-event sequence or the
non-streamed response object. Chat Completions has this (`IngressStream::new/chunk/close` and
`encode_ingress_completion`, `crates/llm-chat/src/ingress.rs:370,418,445,494`).

## Design

Mirror the Chat encoder in `b10x-llm-responses` (new module, exported from `lib.rs`):

- `encode_ingress_response(outcome, id, created_at) -> Result<Value, Error>`: the non-streamed
  response object.
- `IngressStream::new(id, created_at)`; `chunk(&StreamEvent) -> Vec<Value>` (one neutral delta may
  need several wire events, for example the item and part that open the first text delta);
  `close(&TurnOutcome) -> Result<Vec<Value>, Error>` (the done events and exactly one terminal
  `response.completed` or `response.incomplete`); `fail(&Error) -> Vec<Value>` (the terminal
  `response.failed`). Each value is one event payload carrying its `type`; SSE framing stays with the
  server.
- Wire coverage: text (`response.output_text.delta`), tool calls (`function_call` items, from
  `ToolCallStarted` then `response.function_call_arguments.delta`), reasoning deltas on a reasoning
  item, every `StopReason`, usage with only reported counters (an unknown counter is left out, never
  zero), `upstream_model` left out when unreported. Opaque state the wire can carry is carried; what
  it cannot carry is refused, never dropped. An outcome that contradicts what was streamed is
  refused, as Chat's `close` does. The caller supplies ids and the clock.
- Emit only what this repository's decoder (`llm_responses::decode_stream`) accepts:
  `ACCEPTED_STREAM_EVENTS` and docs/responses.md "The pinned stream subset".

## Shared files

This story edits, besides `crates/llm-responses` and its tests: `spec/domains/responses.yaml` (the
new command and observation), `checks/conformance/src/responses.rs`, `contracts/responses/scenarios/`
and its own lines in `contracts/ess-inputs.yaml` (the `responses/scenarios/` block).
story:routes-document and story:messages-ingress-encoder append to the same `contracts/ess-inputs.yaml`
in their own domains' blocks. `contracts/suite.json`, `contracts/schema/` and
`contracts/baseline.json` are regenerated or raised in the branch only to run the check there; the
coordinator owns them at integration: it resolves `ess-inputs.yaml`, regenerates the projections
and sets the baseline once on the merged wave. The conformance acceptance below is read on the
merged wave.
The CHANGELOG.md (Unreleased), README.md, AGENTS.md, docs/implementation-status.md with `task docs-generate`, the hand-written pages (docs/responses.md, docs/messages.md, the routes page) and the website pages are written by the coordinator once on the integration branch after the three stories merge, not in this story's branch.

## Acceptance

- Round trip, streamed: a Responses request body ingested with `ingest_request` yields a
  `TurnRequest`; synthetic `StreamEvent`s for text, a reasoning delta and two tool calls plus a
  `TurnOutcome` encoded with `IngressStream` and decoded with `decode_stream` on the same binding give
  the same events and a `TurnOutcome` equal to the encoded one, for each of `EndTurn`, `ToolCalls`,
  `MaxOutputTokens` and an `Incomplete` reason.
- Round trip, non-streamed: `encode_ingress_response`, wrapped as the decoder's terminal object,
  decodes to an equal outcome.
- A usage with an unknown input count encodes without `input_tokens` and decodes back unknown, never
  zero.
- `fail` produces a `response.failed` the decoder turns into the error's code.
- An outcome contradicting the streamed calls, and caller-owned items, are refused.
- Conformance: scenarios in `contracts/responses/scenarios/` for a new `llm.responses` command (for
  example `llm.responses.EncodeIngress`, mirroring `llm.chat.EncodeIngress`), passing in `task conformance` on the merged wave with no skips,
  `contracts/baseline.json` raised by their count.

## Verification

`cargo test -p b10x-llm-responses`; `cargo clippy -p b10x-llm-responses --all-targets -- -D
warnings`; `task conformance`. No network I/O.
