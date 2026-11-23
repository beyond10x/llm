---
format: aep.planning-md/3
id: story:final-only-answer-text
kind: story
status: implemented
title: Answer text that arrives only in the final object reaches the caller once
summary: llm infer and the Chat relay dropped an assistant message no text delta showed; each now reaches the caller exactly once
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-chat/src/ingress.rs
- confidence: inferred
  path: crates/llm-chat/tests/ingress.rs
- confidence: cited
  path: crates/llm-cli/src/lib.rs
- confidence: inferred
  path: crates/llm-cli/tests/command.rs
- confidence: cited
  path: crates/llm-core/src/port.rs
- confidence: inferred
  path: crates/llm-core/tests
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-23T02:01:38Z", actor: "agent:llm-u1-implementor", revision: 4}
- {from: "proposed", to: "active", at: "2026-11-23T02:01:39Z", actor: "agent:llm-u1-implementor", revision: 5}
- {from: "active", to: "implemented", at: "2026-11-23T02:01:44Z", actor: "agent:llm-u1-implementor", revision: 6, decided_on: {"recorded":{"test_result":1}}}
---
## Source

Review of llm 0.24.0 (`08289a72`), 2026-11-23, finding 2: "A completed answer can reach the CLI with zero
bytes on stdout. The released CLI exited 0 and reported 12 input and 4 output tokens when the only answer
text arrived in the final Responses object. With one streamed message and a second final-only message, it
printed only the first. The Chat Completions gateway encoder also emitted no text for the final-only turn,
although its ordinary JSON encoder returned the full answer. Reconcile each final message with any text
already shown so it reaches the caller once."

The cause: a Responses turn may carry an assistant message no `response.output_text.delta` showed (the
server sent it only in the terminal `output`). The decoder keeps it in the outcome with the server's text,
as `StreamEvent` documents, but emits no event for it. `llm infer` writes only `TextDelta` and
`RefusalDelta` events to stdout and never reads the outcome's text, and `llm_chat::IngressStream::close`
sends only calls, the finish reason and usage, so both dropped that message.

## Outcome

One rule, in `llm-core`, says which of a turn's assistant messages its stream showed: the text deltas
between message boundaries are the shown messages, and each is paired with one message of the outcome
that carries exactly that text. Every other non-empty assistant message (and refusal) of the outcome was
not shown. `llm infer` writes each message that was not shown to stdout after the stream, in outcome
order, each on its own line after the text before it; a shown message is never written twice.
`llm_chat::IngressStream::close` sends the text of each message that was not shown as `content` (and a
refusal as `refusal`) before its finish chunk, joined to the text already sent with a newline, as
`encode_ingress_response` joins messages. A final-only message is not a replacement of shown text: the
invariant "a streamed delta is final" and its tests are unchanged.

## Acceptance

1. `llm infer` on a Responses route whose answer arrives only in the terminal `output` prints that text on
   stdout, ending with a newline, and exits 0.
2. `llm infer` on a Responses route with one streamed message and a second final-only message prints both
   texts, each exactly once.
3. `llm_chat::IngressStream`, fed the events of a final-only Responses turn and closed on its outcome,
   sends chunks whose concatenated `content` equals the `content` `encode_ingress_response` returns for
   the same outcome; with one streamed and one final-only message the streamed text is sent once.
4. A turn whose every message was streamed prints and relays exactly what it did before: the existing
   CLI and Chat ingress tests pass unchanged.
5. `crates/llm-responses/tests/live_stream.rs` passes unchanged.
6. `CHANGELOG.md` says so under Unreleased.

## Not covered

The Responses relay (`llm_responses::IngressStream::close`) still pairs shown messages with the outcome's
messages by position and refuses a turn whose final-only message precedes a streamed one (review finding
3). The Messages relay refuses the same order, because a block it already streamed fixes its index. A
final-only message printed after streamed text appears after it on stdout even where the turn places it
before: shown text is final and cannot be moved.
