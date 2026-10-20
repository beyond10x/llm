---
format: aep.planning-md/3
id: story:messages-cut-after-silent-output-is-final
kind: story
status: implemented
title: A Messages turn cut after a silent block is final, as on Responses
summary: The Messages client does not clear the retry mark once its decoder has answered; Responses does
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-messages/src/client.rs
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: inferred
  path: crates/llm-messages/tests/retry_class.rs
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: docs/messages.md
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T07:47:11Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T07:47:11Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-20T13:49:44Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

Found by the implementor of `story:retry-classes-harness-retried` in wave 2026-10-20-w09, with a
probe: a Messages transport cut after a silent block (opened, nothing visible streamed) stays
retriable (`cut after silent text: shown=0 code=Protocol dispatch=Accepted retriable=true`). The
Responses client clears the mark once its decoder has answered (`crates/llm-responses/src/client.rs`,
the `Err` arm of its stream loop); the Messages client does not. It has been so since wave w27.

## Outcome

A Messages turn that fails after any output, visible or silent, is final, as on Responses.

## Acceptance

- A test cuts a Messages stream after a silent block and asserts the failure is not retriable;
  a cut before any block stays retriable.
- The fix shape the implementor proposed: a `pub(crate) fn has_answered` on `StreamDecoder`, and
  `with_retriable(false)` in the `Err(error)` arm of `MessagesClient::stream`.

## Scope

Derived 2026-10-20 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Checked claim:** Responses clears the mark in a `map_err` on `stream.next()` when
  `decoder.has_answered()` (`crates/llm-responses/src/client.rs:212-219`, accessor
  `crates/llm-responses/src/stream.rs:427-429`); Messages returns `decoder.attach(error)`
  (`crates/llm-messages/src/client.rs:158`), which never touches the retry mark (`decode.rs:275-277`,
  `:112-124`); the private `answered` flag (`decode.rs:242`, set at `:327`) has no accessor; the cut
  comes from `crates/llm-http/src/sse.rs:170-172` and `crates/llm-http/src/transport.rs:323-325` — cited
- **Primary surface:** `crates/llm-messages` — cited
- **Files:** `crates/llm-messages/src/client.rs:158` (`MessagesClient::stream`), `crates/llm-messages/src/decode.rs:242`, `:270-277` (new `pub(crate) fn has_answered`) — cited
- **Tests:** `crates/llm-messages/tests/retry_class.rs` (or `adversary_w09_retry.rs`), a cut test mirroring `crates/llm-responses/tests/retry_class.rs:108` — inferred
- **Documents:** `docs/messages.md:222-225`; `docs/harness-parity.md` rows H8 and H18 if the next-release section lists the change; `AGENTS.md:50`; `CHANGELOG.md` — inferred
- **Confidence:** high
- **Would collide with:** the Messages stream decoder (`decode.rs`, additive accessor) and `crates/llm-messages/src/client.rs:121-171`; no code shared with the Messages ingress encoder (`crates/llm-messages/src/ingress.rs`); shares `docs/messages.md`, `CHANGELOG.md`, `AGENTS.md` — cited
- **Safety fact:** clearing the mark on an `Accepted` error turns routing's same-target retry (`crates/llm-routing/src/fallback.rs:527`) into a halt and opens no fallback (`fallback.rs:230-235`); a cut before any block keeps `answered` false (`decode.rs:217`, `:327`) — walked, not run

## Design decisions

Taken 2026-10-20 by the wave's coordinator inside the approved design:

- **Chat joins the unit.** The implementor found the same unconditional `Err` arm in
  `crates/llm-chat/src/client.rs` and measured it: a Chat stream cut after output read
  `retriable: true`. It is fixed the same way (an accessor on the Chat decoder, the mark cleared
  once it has answered), with its own tests. Chat counts non-empty `content` or
  `reasoning_content`, any `tool_calls` fragment and a finish reason as output; the opening `role`
  chunk with empty or null content does not.
- **Left as they are.** `decode_stream` (`crates/llm-messages/src/decode.rs`) and Chat's
  `project_response_bytes` keep a retriable cut after output: their only callers outside tests are
  the conformance checks, and no local consumer checkout calls them.
