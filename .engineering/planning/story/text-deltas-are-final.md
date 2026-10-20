---
format: aep.planning-md/3
id: story:text-deltas-are-final
kind: story
status: active
title: A streamed text delta is never revised, and a caller is told when the outcome's text differs from what it was shown
summary: Nothing states that a TextDelta is final; on Responses the outcome's message outranks the deltas the caller was shown
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts/responses/scenarios/kept-text-a-terminal-message-outranks-the-shown-deltas.yaml
- confidence: inferred
  path: contracts/suite.json
- confidence: inferred
  path: crates/llm-chat/tests/incoming.rs
- confidence: cited
  path: crates/llm-core/src/port.rs
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: inferred
  path: crates/llm-messages/tests/streaming.rs
- confidence: cited
  path: crates/llm-responses/src/stream.rs
- confidence: inferred
  path: crates/llm-responses/tests/live_stream.rs
- confidence: cited
  path: docs/chat.md
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: docs/harness-parity.md
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: docs/responses.md
- confidence: inferred
  path: spec/domains/responses.yaml
- confidence: inferred
  path: website/docs/concepts/neutral-boundary.md
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T01:03:33Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T01:03:33Z", actor: "human:timo", revision: 6}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100). Its LLM node commits text at a commit
point, and text-to-speech speaks committed text; spoken text cannot be taken back. inference treats
every `StreamEvent::TextDelta` as final because its commit point depends on it.

At tag 0.7.0 nothing states that. `StreamEvent` (`crates/llm-core/src/port.rs:10-35`) has no event
that retracts or replaces earlier text, so a delta is final within the stream by vocabulary, but no
document or test says so and a later event kind could change it.

A second, related fact: on Responses the turn's outcome can carry text that differs from the deltas
the caller was shown. `keep_shown_text` (`crates/llm-responses/src/stream.rs:296-309`) documents that
"a message the turn carries for the same output item outranks that item's deltas, which are then only
what was shown on the way". A caller that spoke the deltas and then stores the outcome in its history
stores text it did not speak, and nothing tells it so. Responses has a second route to the same
divergence: with an empty terminal `output`, the outcome takes the `response.output_item.done`
message object, which also outranks the deltas (`stream.rs:201-217`, `:378-388`).

Checked while scoping, by reading, not run:

- Messages: the outcome's text is the block's `content_block_start` text plus its deltas
  (`crates/llm-messages/src/decode.rs:407-412`, `:453-461`, `:491`), and the start text is never
  emitted as a delta. A start of `{"type":"text","text":"X"}` gives an outcome beginning with `X`
  that the caller never saw. All 9 fixtures send `"text":""`; whether a real route sends non-empty
  start text is not known.
- Chat: the outcome's text is the deltas' concatenation (`crates/llm-chat/src/incoming.rs:126-129`,
  `:244-245`); it cannot differ.

## Outcome

The port's documentation states that a text delta is never revised by a later event, and a caller
can tell when a turn's outcome carries assistant text that differs from the deltas it was shown.

## Acceptance

- The `StreamEvent` and `Model` documentation states that emitted text is final: no later event
  retracts or replaces it. A test per protocol (Responses, Messages, Chat) feeds a stream and asserts
  the concatenated deltas are a prefix of, or equal to, what the caller can observe of the turn.
- Where an outcome's assistant text differs from the deltas shown for the same item (the Responses
  case above, and any found in Messages or Chat), the caller is told: a `StreamEvent::Warning` with
  a fixed code, or a field on the outcome; the choice is recorded in `docs/design.md`.
- `docs/responses.md`, `docs/messages.md` and `docs/chat.md` say which holds for their protocol.

## Scope

Derived 2026-10-20 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-responses` (`src/stream.rs`, the confirmed divergence) — cited
- **Files:** `crates/llm-core/src/port.rs:10-52` (`StreamEvent`, `Model` docs) — cited
- **Files:** `crates/llm-responses/src/stream.rs:296-328` (`keep_shown_text`; outranking filter `:312-318`) — cited
- **Files:** `crates/llm-messages/src/decode.rs:390-466` (`start_block` seeds a text block with unshown `content_block.text`, `:407-412`, `:453-461`) — cited
- **Tests:** `crates/llm-responses/tests/live_stream.rs` — inferred, home of the R36 kept-text tests
- **Tests:** `crates/llm-messages/tests/streaming.rs` — inferred, home of the stream decoder tests
- **Tests:** `crates/llm-chat/tests/incoming.rs` — inferred, home of the stream projection tests; Chat needs no decoder change
- **Symbols:** `StreamEvent`, `Model`, `Decoder::keep_shown_text`, `Decoder::terminate`, `StreamDecoder::start_block`, `StreamDecoder::apply_delta`, `StreamProjection::delta`, `StreamProjection::outcome` — cited
- **Also likely:** `spec/domains/responses.yaml` ("Kept text", `:45-55`, states the outranking rule) — inferred
- **Also likely:** `contracts/responses/scenarios/kept-text-a-terminal-message-outranks-the-shown-deltas.yaml` (asserts `stream_events: [text-delta:a draft]`; a warning adds `warning:<code>` per `checks/conformance/src/responses.rs:491`) — inferred
- **Also likely:** `contracts/suite.json` (generated; regenerated if that scenario changes or scenarios are added) — inferred
- **Also likely:** `website/docs/concepts/neutral-boundary.md` ("Streamed events", `:21-26`) — inferred
- **Also likely:** `docs/harness-parity.md` row R36 describes the outranking behaviour — inferred
- **Documents:** `docs/design.md` ("Neutral boundary" `:13-19`), `docs/responses.md` (`:318-327`), `docs/messages.md` (`:99-137`), `docs/chat.md` (`:53-75`) — cited
- **Confidence:** medium — the Messages fix site depends on an unmade choice (emit, refuse or warn on start text), and the "field on the outcome" option would widen scope to every `TurnOutcome {` constructor (30 files, 8 crates plus `checks/` and `crates/llm-docs/examples/`) and make `TurnOutcome` (`deny_unknown_fields`, `crates/llm-core/src/turn.rs:348-354`) a serialized format change
- **Would collide with:** any unit touching `crates/llm-responses/src/stream.rs`, `crates/llm-messages/src/decode.rs`, `crates/llm-core/src/port.rs`, `docs/{responses,messages,chat}.md`, `docs/design.md`, or `contracts/suite.json` — inferred
- **Safety fact:** a `StreamEvent::Warning` raised while deciding the turn reaches the caller with no client change, and no ingress encoder relays it: the Responses client hands `finish()`'s events to the sink before returning the outcome (`crates/llm-responses/src/client.rs:150`), and the ingress encoders drop `Warning` (`crates/llm-responses/src/ingress.rs:248`, `crates/llm-chat/src/ingress.rs:612`, `crates/llm-messages/src/ingress.rs:192`) — cited, unproven

## Design decisions

Taken 2026-10-20 from `docs/plan/wave-2026-10-20-w09-design-review.md` § Layer 2 (2.1 to 2.4).

- **The shown deltas are the outcome's text.** Each assistant item's text in a successful outcome is
  the concatenation of the `TextDelta`s shown for it. Where a server's own final text differed and
  was replaced, the decoder emits `StreamEvent::Warning` with the fixed code `shown-text-kept` before
  returning the outcome. No `TurnOutcome` field and no new `StreamEvent` variant.
- **Responses:** one post-pass in the decoder's `terminate` covering both routes
  (`keep_shown_text`, `crates/llm-responses/src/stream.rs:312-318`, and an empty terminal `output`
  taking the `output_item.done` message, `:201-217`, `:366-387`), pairing by item id where both
  sides have one and by order otherwise. Harness's R36 kept text is unchanged.
- **Messages:** text in `content_block_start` is emitted as a `TextDelta` at block start and folded
  into the block as today (`crates/llm-messages/src/decode.rs:407-461`), as `thinking_delta` is.
- **Chat:** cannot differ (`crates/llm-chat/src/incoming.rs:126-129`, `:244-245`); a test pins it.
- **Guarantee and tests:** the `StreamEvent` and `Model::turn` docs state that every event is final
  and the rule above. Per protocol: deltas equal outcome text; `shown-text-kept` appears exactly
  when text was replaced; the same events fed to that protocol's ingress encoder close on the
  outcome without error. The scenario
  `contracts/responses/scenarios/kept-text-a-terminal-message-outranks-the-shown-deltas.yaml` flips
  to the shown text and gains the warning; `spec/domains/responses.yaml:45-55`,
  `docs/responses.md:318-327` and `docs/harness-parity.md` row R36's note change with it.
