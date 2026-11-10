---
format: aep.planning-md/3
id: story:ingress-block-boundaries
kind: story
status: implemented
title: A Messages relay keeps redacted thinking between streamed blocks
summary: StreamEvent has no block boundary, so the Messages ingress encoder refuses an answer with redacted thinking between streamed blocks
relations:
- decomposes: epic:inference
- informed_by: review-result:adversary-w07-messages-ingress-encoder-pass-1
- informed_by: review-result:adversary-w07-responses-ingress-encoder-pass-1
- informed_by: review-result:adversary-w07-responses-ingress-encoder-pass-2
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: checks/conformance/src/bedrock.rs
- confidence: cited
  path: checks/conformance/src/chat.rs
- confidence: cited
  path: checks/conformance/src/messages.rs
- confidence: cited
  path: checks/conformance/src/responses.rs
- confidence: inferred
  path: contracts/messages/scenarios
- confidence: inferred
  path: contracts/responses/scenarios
- confidence: inferred
  path: contracts/suite.json
- confidence: inferred
  path: crates/llm-bedrock/src/converse.rs
- confidence: cited
  path: crates/llm-bedrock/tests/converse.rs
- confidence: cited
  path: crates/llm-chat/src/ingress.rs
- confidence: cited
  path: crates/llm-core/src/port.rs
- confidence: inferred
  path: crates/llm-messages/src/decode.rs
- confidence: cited
  path: crates/llm-messages/src/ingress.rs
- confidence: cited
  path: crates/llm-messages/tests/adversary_w07_ingress.rs
- confidence: inferred
  path: crates/llm-messages/tests/ingress.rs
- confidence: inferred
  path: crates/llm-messages/tests/streaming.rs
- confidence: cited
  path: crates/llm-responses/src/ingress.rs
- confidence: inferred
  path: crates/llm-responses/src/stream.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w07_ingress.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w07_ingress_pass2.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w09_shown_text.rs
- confidence: inferred
  path: crates/llm-responses/tests/ingress.rs
- confidence: inferred
  path: crates/llm-responses/tests/projection.rs
- confidence: cited
  path: docs/design.md
- confidence: cited
  path: docs/implementation-status.md
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: docs/responses.md
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/concepts/neutral-boundary.md
- confidence: inferred
  path: website/docs/status.mdx
revision: 38
transitions:
- {from: "draft", to: "proposed", at: "2026-11-09T22:06:24Z", actor: "human:timo", revision: 36}
- {from: "proposed", to: "active", at: "2026-11-09T22:06:24Z", actor: "human:timo", revision: 37}
- {from: "active", to: "implemented", at: "2026-11-10T04:21:38Z", actor: "human:timo", revision: 38, decided_on: {"recorded":{"test_result":1,"review_outcome":4}}}
---
## Context

Found by the adversaries of wave 2026-10-16-w07. `StreamEvent` carries no item or block boundary,
so two ingress encoders refuse an answer whose items are not separated by an event of another kind:

- Messages (review-result:adversary-w07-messages-ingress-encoder-pass-1, `crates/llm-messages/src/ingress.rs:384`):
  an answer of [thinking, redacted_thinking, text] decoded by `llm_messages::StreamDecoder` and
  re-encoded by `llm_messages::IngressStream` is refused at `close`, after the text was shown;
  `redacted_thinking` emits no `StreamEvent`, so the text opens at the index the redacted block held.
- Responses (review-result:adversary-w07-responses-ingress-encoder-pass-1, `crates/llm-responses/src/ingress.rs:239`):
  two consecutive `TextDelta`s whose outcome holds two `AssistantText` items cannot be split into
  two message items, so `close` refuses the outcome as contradicting the stream; likewise one
  message whose text deltas a reasoning delta or a call interrupts (pass 2,
  review-result:adversary-w07-responses-ingress-encoder-pass-2, `ingress.rs:186,214`).

## Outcome

A relay of such answers round-trips: either `StreamEvent` gains an item boundary (a
neutral-boundary change in `b10x-llm-core`) or the encoders can place unstreamed blocks without
contradicting the stream.

## Acceptance

`crates/llm-messages/tests/adversary_w07_ingress.rs::a_relayed_answer_with_redacted_thinking_reads_back_whole`
and the `two_messages` iteration in `crates/llm-responses/tests/adversary_w07_ingress.rs` assert the
round trip (today they assert the refusal) and pass.

## Scope

Derived 2026-11-09 by `aep:story-scoper` at `7c56b19b`; confidence medium. Nothing was run.

Route: the scoper read the encoder-only route as unable to pass either acceptance test (inferred):
the Messages decoder refuses a block start with a lower index than one already opened
(`crates/llm-messages/src/decode.rs:460`), and on Responses both deltas already went out under one
`item_id`. That leaves the `StreamEvent` boundary, which is a breaking change: `StreamEvent`
(`crates/llm-core/src/port.rs:32-34`) is not `#[non_exhaustive]`, as 0.20.0's `RefusalDelta` was
(`CHANGELOG.md:76`).

- cited: `crates/llm-core/src/port.rs` (`StreamEvent`), `crates/llm-messages/src/ingress.rs`,
  `crates/llm-responses/src/ingress.rs`, the two acceptance tests
  `crates/llm-messages/tests/adversary_w07_ingress.rs` and
  `crates/llm-responses/tests/adversary_w07_ingress.rs`.
- cited, compile-forced exhaustive matches: `crates/llm-chat/src/ingress.rs:752`,
  `checks/conformance/src/{bedrock,chat,messages,responses}.rs`,
  `crates/llm-bedrock/tests/converse.rs:264`.
- cited: `crates/llm-responses/tests/adversary_w07_ingress_pass2.rs` and
  `adversary_w09_shown_text.rs` pin today's refusal by this story's id.
- inferred: `crates/llm-messages/src/decode.rs` (emit at `content_block_start`),
  `crates/llm-responses/src/stream.rs` (emit at `response.output_item.added`), exact-event tests in
  `crates/llm-messages/tests/{ingress,streaming}.rs` and `crates/llm-responses/tests/{ingress,projection}.rs`,
  `crates/llm-bedrock/src/converse.rs`, `contracts/{messages,responses}/scenarios`,
  `contracts/suite.json`, `website/docs/concepts/neutral-boundary.md` and the generated status pages.
- cited documents: `CHANGELOG.md`, `docs/messages.md`, `docs/responses.md`, `docs/design.md`
  ("Streamed events are final"), `docs/implementation-status.md`.
- `spec/` models stream events as strings only, so no `spec/` or `contracts/schema/` change is forced.
- Safety fact (unproven): `crates/llm-routing/src/fallback.rs:226` counts every event as visible
  output and `crates/llm-calls/src/lib.rs:98` starts the first-output clock on anything but
  `Warning`, so a boundary emitted before the first real output would end fallback eligibility.
- Not established: the variant's shape (a bare boundary does not carry the redacted block's data),
  which decoders emit it and when, and whether consumers match `StreamEvent` exhaustively.
