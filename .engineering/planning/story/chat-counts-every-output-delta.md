---
format: aep.planning-md/3
id: story:chat-counts-every-output-delta
kind: story
status: draft
title: A Chat Completions stream counts every text delta as output and returns its turn at [DONE]
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 1
---
## Context

Found by the adversary of `story:messages-cut-after-silent-output-is-final` in wave 2026-10-20-w10
(`review-result:adversary-w10-messages-cut-pass-1`); both hold at the base `9cd7dbe8`.

- **A closed list of output fields.** The Chat Completions decoder counts a chunk as output only
  for non-empty `content` or `reasoning_content`, a `tool_calls` fragment or a finish reason
  (`crates/llm-chat/src/incoming.rs:134-156` at `d8f79616`). A delta carrying `refusal` text, or
  `reasoning` (the name some servers use for `reasoning_content`), is not counted, so a stream cut
  after it stays retriable with nothing shown and routing retries a turn the model already
  answered. Whether the projection carries or drops that text was not established; the workspace
  rule is that an unsupported field is carried or refused, never dropped. OpenAI's wire has the
  `refusal` key: the repository's fixture `crates/llm-chat/fixtures/openai-text-and-usage.sse:1`
  pins it as null. Whether a consumer's server sends it non-empty is not known.
- **Reading past `[DONE]`.** The Chat client's stream loop keeps reading after the `[DONE]` sentinel
  (`crates/llm-chat/src/client.rs:83-108`), so a connection dropped before the chunked body's
  terminating chunk fails a complete turn (`transport`, accepted, not retriable since `d8f79616`;
  retriable before). The Messages client returns the turn once its decoder is complete.

Two red cases that show both are kept outside the repository for the implementor
(`red-chat-cases.rs` in the wave's scratch).

## Outcome

A Chat Completions stream counts every delta that carries text as output, carries or refuses each
such field, and returns a complete turn once `[DONE]` arrives.

## Acceptance

- A cut after a non-empty `refusal` or `reasoning` delta is final; a cut after only `role`, an empty
  `tool_calls` array, empty content with a null finish reason, a null `reasoning_content` or a
  usage-only chunk stays retriable.
- `refusal` and `reasoning` text are carried into the turn (or refused with a named field), never
  dropped; `docs/chat.md` says which.
- A body dropped after `[DONE]` returns the complete turn, as Messages does after its terminal
  event.
