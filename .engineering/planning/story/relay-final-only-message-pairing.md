---
format: aep.planning-md/3
id: story:relay-final-only-message-pairing
kind: story
status: implemented
title: A relay sends a final-only message wherever the turn places it
summary: The Responses and Messages relays refused a turn whose final-only message came before a streamed one; both now pair by ShownText
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- informed_by: story:final-only-answer-text
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-messages/Cargo.toml
- confidence: cited
  path: crates/llm-messages/src/ingress.rs
- confidence: cited
  path: crates/llm-messages/tests/ingress.rs
- confidence: cited
  path: crates/llm-responses/src/ingress.rs
- confidence: cited
  path: crates/llm-responses/tests/ingress.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-23T11:56:25Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":3}}}
- {from: "proposed", to: "active", at: "2026-11-23T11:56:25Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":3}}}
- {from: "active", to: "implemented", at: "2026-11-23T11:56:26Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":3}}}
---
## Source

Review of llm 0.24.0 (`08289a72`), 2026-11-23, finding 3: "The Responses relay can reject a turn its own
decoder accepted. In a tagged fixture, only the second message streamed. The successful final result
contained 'preamble' followed by 'answer'. Replaying the events and result through the release's gateway
encoder returned a protocol error because it compared the streamed text with the first message. This
needs a round-trip check that covers final-only messages before a streamed message."

`story:final-only-answer-text` (0.25.0) left the same gap open on both relays under "Not covered": the
Responses relay (`llm_responses::IngressStream::close`, `check_streamed`) pairs shown messages with the
outcome's messages by position, and the Messages relay (`llm_messages::IngressStream::close`) requires
the outcome to begin with the streamed blocks, so both refuse a turn whose final-only message precedes a
streamed one.

## Outcome

Both relays decide which of the outcome's messages the stream showed with `llm_core::ShownText`, the rule
the Chat relay and `llm infer` already use. A shown message is finished where it was streamed and never
sent again; a message no delta showed is sent once, wherever the turn places it. The Responses relay
keeps the turn's order: every item carries its own `output_index` and `id`, so its decoder reads back the
upstream turn. The Messages relay sends a message no delta showed after every streamed block, because a
streamed block's index is fixed and shown text is final; its caller reads that message after the text it
was shown. Shown text the outcome carries no message for is still refused as a contradiction.

## Acceptance

1. A Responses upstream turn whose first message is final-only and whose second message streamed
   ("preamble" then "answer"), decoded by `llm_responses::decode_stream` and relayed through
   `llm_responses::IngressStream`, closes, and the relayed answer decodes back to the upstream turn
   (finding 3).
2. The same holds, on the Responses relay, for a streamed message before a final-only one, for a turn
   whose every message streamed, and for a turn whose every message is final-only.
3. In each of these four cases the relayed Responses stream shows the streamed text once, exactly as the
   upstream showed it, and shows each final-only message's text once after it.
4. The same four upstream turns relayed through `llm_messages::IngressStream` close, and the relayed
   answer decodes back to a turn carrying every message once: the streamed blocks first, then each
   final-only message in the upstream turn's order; the streamed text is not sent again.
5. An empty message, which no delta shows either, before or after a streamed one closes on the Messages
   relay and reads back once after the streamed blocks (the Responses relay already relayed it in place).
6. Shown text the outcome carries no message for, and a shown message the outcome carries with other
   text, are still refused with a `protocol` error on both relays.
7. The invariant "A streamed delta is final" and its tests in `AGENTS.md` pass unchanged.
8. `CHANGELOG.md` says so under Unreleased.

## Not covered

Content other than a message that the Messages stream never showed (a call, signed thinking) placed
between two streamed blocks is still refused, as `llm_messages::ingress` documents.
