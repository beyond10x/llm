---
format: aep.planning-md/3
id: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
kind: story
status: draft
title: A Bedrock Converse turn that stops for an unmodelled reason without content is a classified failure
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w30-messages-empty-turn-pass-1
- decomposes: epic:bedrock
revision: 1
---
## Context

Wave 2026-11-25-messages-empty-turn made a Messages answer that stops for an unmodelled reason
without content a classified error (issue 83). That rule lives in `Header::outcome` of
`crates/llm-messages/src/decode.rs`, which a Messages stream, a complete message and Bedrock
Anthropic `InvokeModel` share.

Bedrock Converse has its own copy of the mapping: `stop_reason` in
`crates/llm-bedrock/src/converse.rs` turns every reason other than `end_turn`, `tool_use` and
`max_tokens` (`stop_sequence`, `guardrail_intervened`, `content_filtered`,
`model_context_window_exceeded` and any other) into `StopReason::Incomplete { reason }`, and the
outcome is accepted with no items. The wave's implementor read the file and reported the same defect;
the wave did not change it.

## Outcome

A Bedrock Converse turn that stopped for an unmodelled reason and decoded to no item returns an
`Error` the caller can classify, with the reported usage attached, by the same table as Messages.
With an item, `StopReason::Incomplete { reason }` still stands.

## Open

Which error code `guardrail_intervened` and `content_filtered` map to without content: `refused`, as
Messages `refusal` does, or their own reading.

## Acceptance

- A Converse answer, streamed and whole, ending in `model_context_window_exceeded` with no item
  returns `too-large`, with its usage.
- A Converse answer ending in a reason in no list with no item returns a retriable `unavailable`,
  with its usage, bounded by the retry policy.
- The same answers after a text block return an outcome with `StopReason::Incomplete { reason }`.
- The retry rule of `docs/design.md` and the Bedrock retry row of the invariants table in `AGENTS.md`
  name the case.

## Verification

Targeted: `-p llm-bedrock` named test targets and the conformance check. No live call.
