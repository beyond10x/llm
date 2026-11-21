---
format: aep.planning-md/3
id: story:empty-signed-reasoning
kind: story
status: implemented
title: Reasoning that is signed but shows no text streams as a thinking block before the answer
summary: An empty ReasoningDelta marks it; IngressStream opens the block and close signs it in place
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-core/src/port.rs
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: cited
  path: crates/llm-messages/src/ingress.rs
- confidence: cited
  path: crates/llm-messages/tests/ingress.rs
- confidence: cited
  path: docs/messages.md
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-21T04:34:24Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-21T04:34:24Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-21T05:15:23Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}}
---
## Source

The operator, 2026-11-21, about llm-gateway dropping behaviour: "we need to fix the LLM gateway then. You can
just do that in this session."

A deployment whose backend sends Anthropic Messages requests through llm-gateway to Bedrock measured it
(gateway 0.23.0 on llm 0.20.0): Claude Haiku 5.5 at effort `low`, with no `thinking` field in the request,
opens its answer with a signed `thinking` block whose text is empty (`content_block_start` with
`"thinking":""`, one `signature_delta`, `content_block_stop`), then its text or tool call. The gateway relayed
the text at index 0 and then ended the stream with an `error` event: `IngressStream::close` found the
thinking item at index 0 of the outcome and refused it as a contradiction (`settle`, the `_` arm). The
cause is this repository's: `StreamDecoder` shows nothing for a thinking block without text
(`start_block`: `"thinking" => None`; a `thinking_delta` or `signature_delta` shows nothing either), and
`IngressStream::chunk` opens a thinking block only on a non-empty `ReasoningDelta`, so a re-encoder cannot
know the block is there before the answer after it.

## Outcome

A thinking block that the stream decoder signs without having shown any of its text is marked by one
empty `ReasoningDelta` at its first `signature_delta`. `IngressStream` opens a thinking block, with empty
text, on an empty `ReasoningDelta` when reasoning is relayed and the open block is not already thinking,
and sends no `thinking_delta` for it; `close` signs it in place. A non-empty reasoning stream, and a
stream built `without_reasoning_relay`, are unchanged.

## Acceptance

1. Empty signed reasoning then text round-trips through `IngressStream` and the decoder, the thinking
   block at index 0 and the text at index 1.
2. Empty signed reasoning then a tool call round-trips the same way.
3. Empty signed reasoning alone, ending by `max_tokens`, round-trips.
4. An upstream Messages answer of that shape, decoded by `decode_stream` and re-encoded from the events
   it showed, reads back as the same outcome with the same shown events.
5. Without reasoning relay an empty reasoning delta opens nothing and the old contradiction stands; the
   existing ingress round trips with non-empty reasoning pass unchanged.
6. `docs/messages.md` and `CHANGELOG.md` say so.

## Not covered

A tool call cut off by `max_tokens` (its streamed arguments are not JSON) is still refused at
`content_block_stop` ("Messages streamed tool arguments are not JSON"); the neutral `ToolCall` has no form
for arguments that are not JSON, so relaying it needs a design decision of its own.
