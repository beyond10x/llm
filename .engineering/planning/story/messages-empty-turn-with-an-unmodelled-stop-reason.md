---
format: aep.planning-md/3
id: story:messages-empty-turn-with-an-unmodelled-stop-reason
kind: story
status: draft
title: A Messages turn that stops for an unmodelled reason without content is a classified failure (issue 83)
tags:
- from-issue
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 1
---
## Context

Issue 83 of this repository (opened 2026-11-14), read at `main` `c7c2a62cb` on 2026-11-24. A Messages
stream whose `message_delta` names a stop reason the decoder does not model, and that carried no
content block, decodes to a successful `TurnOutcome` with `StopReason::Incomplete { reason }` and no
items. The issue cites `crates/llm-messages/src/decode.rs:159-169` (every reason other than `end_turn`,
`tool_use` and `max_tokens` becomes `Incomplete`), `decode.rs:142-155` (`Header::outcome` accepts it
without content) and `crates/llm-core/src/turn.rs:388-452` (`TurnOutcome::validate_for`). The two
reasons it names: `model_context_window_exceeded` without content, and a reason in no list without
content. A caller that reads `Ok(outcome)` as an answer gets an empty one it cannot retry or route on.

Filed by the coordinator from the issue while planning the waves of 2026-11-24. The lines above are
the issue's; the coordinator did not re-read the cited code.

## Outcome

Decoding a Messages turn that stopped for an unmodelled reason and produced no content returns an
`Error` the caller can classify, with the reported usage attached. With content,
`StopReason::Incomplete { reason }` still stands.

## Open

Which error code each reason maps to. The issue proposes "too large" for
`model_context_window_exceeded` and a provider failure that is retriable before output for an unlisted
reason; it reports a second client implementation that fails both as overloaded.

## Acceptance

- A stream ending in `model_context_window_exceeded` with no content block returns the error the Open
  section settles, with its usage.
- A stream ending in an unlisted reason with no content block returns a provider failure that is
  retriable, with its usage.
- The same two streams after a text block return an outcome with `StopReason::Incomplete { reason }`.
- The issue is closed with the release that carries it.

## Verification

Targeted: `-p llm-messages` named test targets and the conformance check. No live call.
