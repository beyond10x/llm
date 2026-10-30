---
format: aep.planning-md/3
id: story:chat-counts-every-output-delta
kind: story
status: active
title: A Chat Completions stream counts every text delta as output and returns its turn at [DONE]
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: crates/llm-chat/src/client.rs
- confidence: cited
  path: crates/llm-chat/src/incoming.rs
- confidence: inferred
  path: crates/llm-core/src/port.rs
- confidence: cited
  path: docs/chat.md
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T21:34:43Z", actor: "human:timo", revision: 7}
- {from: "proposed", to: "active", at: "2026-10-30T17:35:56Z", actor: "human:timo", revision: 8}
---
## Context

Found by the adversary of `story:messages-cut-after-silent-output-is-final` in wave 2026-10-20-w10
(`review-result:adversary-w10-messages-cut-pass-1`); both hold at the base `9cd7dbe8`. File and
line references below are refreshed against `main` at `bb3bda33` (llm 0.17.1).

- **A closed list of output fields.** The Chat Completions decoder counts a chunk as output only
  for non-empty `content` or `reasoning_content`, a `tool_calls` fragment or a finish reason
  (`crates/llm-chat/src/incoming.rs:133-192`: `chunk` sets `answered` at `:155` for a finish
  reason, `delta` at `:170` for `content`, `:178-181` for `reasoning_content`, `:189` for
  `tool_calls`; the rule is stated at `:89-92`). A delta carrying `refusal` text, or `reasoning`
  (the name some servers use for `reasoning_content`), is not counted, so a stream cut after it
  stays retriable with nothing shown and routing retries a turn the model already answered. The
  decoder reads neither key, and response fields it does not read are ignored (`docs/chat.md`
  "Response fields this subset does not read are ignored"), so today that text is dropped. The
  workspace rule is that an unsupported field is carried or refused, never dropped. OpenAI's wire
  has the `refusal` key: the repository's fixture `crates/llm-chat/fixtures/openai-text-and-usage.sse:1`
  pins it as null. Whether a consumer's server sends it non-empty is not known.
- **Reading past `[DONE]`.** The Chat client's stream loop (`crates/llm-chat/src/client.rs:86-111`,
  `Framing::DoneSentinel` at `:80`) leaves only on `Ok(None)` (`:98`); the decoder marks the stream
  terminated at `[DONE]` (`crates/llm-chat/src/incoming.rs`, `framed`, `SseEvent::Done`), but the
  loop keeps reading, so a connection dropped before the chunked body's terminating chunk fails a
  complete turn at `:102-110` (`transport`, accepted, not retriable). The Messages client returns
  the turn once its decoder is complete.

Two red cases that show both are kept outside the repository for the implementor
(`red-chat-cases.rs` in wave 2026-10-20-w10's scratch).

## Decision on `refusal` and `reasoning` deltas (D227)

Decided by the orchestrator (`approval-record:d227-takt-goal-a-llm-drafts`): count them as output
like other output deltas, and carry each through as its own kind; refuse one only if the wire gives
no way to tell it apart. What the coordinator found when writing this down:

- `delta.refusal` is its own key on the OpenAI wire (fixture above), so it is told apart. It has no
  neutral kind today: `llm_core::StreamEvent` (`crates/llm-core/src/port.rs:34-57`) has
  `TextDelta`, `ToolCallStarted`, `ToolArgumentsDelta`, `ReasoningDelta` and `Warning`. Carrying it
  as its own kind means a new `StreamEvent` variant (a breaking change for an exhaustive `match` on
  `StreamEvent`, to be marked so in the CHANGELOG) and a place for the refusal text in the returned
  turn; it never joins the assistant text.
- `delta.reasoning` is told apart from `content`; it is the same kind as `reasoning_content` and is
  carried as `StreamEvent::ReasoningDelta`, as `reasoning_content` is. Inferred, not checked here:
  a server may send both keys in one delta with the same text; then it is carried once. Both keys
  non-empty with different text in one delta is the one case the wire gives no way to tell apart,
  and that delta is refused, naming the field.
- Output usage counters are the server's report and are not changed by this story; "counted" is
  the retry rule: a non-empty `refusal` or `reasoning` delta makes a later cut final.

## Outcome

A Chat Completions stream counts every delta that carries text as output, carries each such field
as its own kind, and returns a complete turn once `[DONE]` arrives.

## Acceptance

- A cut after a non-empty `refusal` or `reasoning` delta is final; a cut after only `role`, an empty
  `tool_calls` array, empty content with a null finish reason, a null `reasoning_content` or a
  usage-only chunk stays retriable.
- `refusal` text reaches the caller as its own `StreamEvent` kind and in the returned turn, never in
  the assistant text; `reasoning` text reaches the caller as `StreamEvent::ReasoningDelta`;
  `docs/chat.md` lists both rows in its wire table.
- A delta whose `reasoning` and `reasoning_content` are both non-empty and differ is refused,
  naming the field; equal text is carried once.
- A body dropped after `[DONE]` returns the complete turn, as Messages does after its terminal
  event.

## Scope

Scoped 2026-10-29 at `bb3bda33`; confidence medium (the neutral kind for `refusal` touches
`llm-core`).

- `crates/llm-chat/src/incoming.rs`: `chunk` (`:133`) and `delta` (from `:165`), the output rule (`:89-111`).
  cited
- `crates/llm-chat/src/client.rs`: the stream loop (`:86-111`). cited
- `crates/llm-core/src/port.rs`: a `StreamEvent` kind for refusal text (`:34-57`), and wherever the
  returned turn holds it (`crates/llm-core/src/item.rs`). inferred
- New tests: `crates/llm-chat/tests/` (a new file for the cut and `[DONE]` cases), and a fixture
  under `crates/llm-chat/fixtures/` with a non-empty `refusal` and a `reasoning` delta. cited
- `docs/chat.md`: the wire table (`:68-76` at `bb3bda33`) and the cut rule (`:297-303`). cited
- Collides with any unit editing `crates/llm-chat/src/incoming.rs` or `crates/llm-core/src/port.rs`.
