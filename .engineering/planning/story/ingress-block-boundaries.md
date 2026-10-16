---
format: aep.planning-md/3
id: story:ingress-block-boundaries
kind: story
status: draft
title: A Messages relay keeps redacted thinking between streamed blocks
summary: StreamEvent has no block boundary, so the Messages ingress encoder refuses an answer with redacted thinking between streamed blocks
relations:
- decomposes: epic:inference
- informed_by: review-result:adversary-w07-messages-ingress-encoder-pass-1
- informed_by: review-result:adversary-w07-responses-ingress-encoder-pass-1
- informed_by: review-result:adversary-w07-responses-ingress-encoder-pass-2
revision: 3
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
