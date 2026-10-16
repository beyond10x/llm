---
format: aep.planning-md/3
id: story:messages-ingress-encoder
kind: story
status: active
title: A server answers a Messages caller from a neutral turn
summary: StreamEvents and TurnOutcome encode to the Messages event stream and message the repo's decoder accepts
relations:
- decomposes: epic:routes-and-ingress
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/messages.rs
- confidence: cited
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/messages/scenarios
- confidence: cited
  path: crates/llm-messages/src
- confidence: cited
  path: crates/llm-messages/tests
- confidence: cited
  path: docs/verification/messages-falsification.json
- confidence: cited
  path: spec/domains/messages.yaml
revision: 12
transitions:
- {from: "draft", to: "proposed", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "proposed", to: "active", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 10, decided_on: {"recorded":{"review_outcome":2}}}
---
## Context

A server answering an Anthropic Messages caller can read the request onto the neutral turn
(`llm_messages::decode_request`, `crates/llm-messages/src/codec.rs`), but cannot answer: nothing turns
`StreamEvent`s and a `TurnOutcome` back into the Messages event stream or the non-streamed message.
Chat Completions has this (`crates/llm-chat/src/ingress.rs:370,418,445,494`).

## Design

Mirror the Chat encoder in `b10x-llm-messages` (new module, exported from `lib.rs`):

- `encode_ingress_message(outcome, id) -> Result<Value, Error>`: the non-streamed message object.
- `IngressStream::new(id)`; `chunk(&StreamEvent) -> Vec<Value>` (the first delta also opens
  `message_start` and a `content_block_start`; a change of block kind stops the open block);
  `close(&TurnOutcome) -> Result<Vec<Value>, Error>` (`content_block_stop` for an open block,
  `message_delta` with `stop_reason` and usage, `message_stop`); `fail(&Error) -> Vec<Value>` (the
  `error` event). Each value is one event payload carrying its `type`; SSE framing stays with the
  server.
- Wire coverage: text (`text_delta`), tool calls (`tool_use` blocks from `ToolCallStarted`, then
  `input_json_delta`), reasoning (`thinking` blocks and `thinking_delta`, with the signature the
  outcome's opaque state carries where it has one; refused where the decoder would refuse it),
  every `StopReason` (`end_turn`, `tool_use`, `max_tokens`, an `Incomplete` reason under its own
  name), usage with only reported counters (an unknown counter left out, never zero), and the
  upstream model left out when unreported. An outcome that contradicts what was streamed is refused.
  The caller supplies ids.
- Emit only what this repository's decoder (`llm_messages::StreamDecoder`, `decode_stream`,
  `decode_message`) accepts; docs/messages.md "What the decoder requires of a stream".

## Shared files

This story edits, besides `crates/llm-messages` and its tests: `spec/domains/messages.yaml` (the
new command and observation), `checks/conformance/src/messages.rs`, `contracts/messages/scenarios/`
and its own lines in `contracts/ess-inputs.yaml` (the `messages/scenarios/` block).
story:routes-document and story:responses-ingress-encoder append to the same
`contracts/ess-inputs.yaml` in their own domains' blocks. `contracts/suite.json`, `contracts/schema/`
and `contracts/baseline.json` are regenerated or raised in the branch only to run the check there;
the coordinator owns them at integration: it resolves `ess-inputs.yaml`, regenerates the
projections and sets the baseline once on the merged wave. The conformance acceptance below is read
on the merged wave.
The CHANGELOG.md (Unreleased), README.md, AGENTS.md, docs/implementation-status.md with `task docs-generate`, the hand-written pages (docs/responses.md, docs/messages.md, the routes page) and the website pages are written by the coordinator once on the integration branch after the three stories merge, not in this story's branch.

## Acceptance

- Round trip, streamed: a Messages request decoded with `decode_request` yields a `TurnRequest`;
  synthetic `StreamEvent`s for text, a reasoning delta and two tool calls plus a `TurnOutcome`
  encoded with `IngressStream` and decoded with `decode_stream` against the same provenance give a
  `TurnOutcome` equal to the encoded one, for each of `EndTurn`, `ToolCalls`, `MaxOutputTokens` and
  an `Incomplete` reason.
- Round trip, non-streamed: `encode_ingress_message` decoded with `decode_message` gives an equal
  outcome.
- A usage with an unknown output count encodes without that counter and decodes back unknown, never
  zero.
- `fail` produces an `error` event the decoder turns into the error's code.
- An outcome contradicting the streamed calls, and caller-owned items, are refused.
- Conformance: scenarios in `contracts/messages/scenarios/` for a new `llm.messages` command (for
  example `llm.messages.EncodeIngress`), passing in `task conformance` on the merged wave with no skips, `contracts/baseline.json` raised by
  their count.

## Verification

`cargo test -p b10x-llm-messages`; `cargo clippy -p b10x-llm-messages --all-targets -- -D
warnings`; `task conformance`. No network I/O.
