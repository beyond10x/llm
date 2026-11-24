---
format: aep.planning-md/3
id: story:messages-empty-turn-with-an-unmodelled-stop-reason
kind: story
status: active
title: A Messages turn that stops for an unmodelled reason without content is a classified failure (issue 83)
tags:
- from-issue
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/messages/scenarios
- confidence: inferred
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: inferred
  path: crates/llm-messages/src/ingress.rs
- confidence: inferred
  path: crates/llm-messages/tests/retry_class.rs
- confidence: cited
  path: crates/llm-messages/tests/streaming.rs
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: docs/verification/messages-falsification.json
- confidence: inferred
  path: spec/domains/messages.yaml
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:00Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:00Z", actor: "human:operator", revision: 3}
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

## Scope

Derived 2026-11-25 by `story-scoper` at `87bc16f16`. Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-messages` — cited
- **Files:** `crates/llm-messages/src/decode.rs` — cited: the check goes where every decode finishes, `Header::outcome` (:166-180), beside `stop_reason` (:183-194)
- **Files:** `crates/llm-messages/tests/streaming.rs` — cited: `an_accepted_field_in_its_documented_shape_is_carried_without_complaint` (:1216) decodes `stop_sequence` with no content block and expects `Incomplete`, so it changes; the new cases fit beside `an_unfinished_stop_reason_is_carried_under_its_own_name` (:956)
- **Files:** `docs/verification/messages-falsification.json` — cited: `crates/llm-messages/tests/falsification.rs` (:119, :151) fails on a new fixed diagnostic in `decode.rs` that is neither claimed nor `unaimed`, and when a recorded `before` text (:744, the `Some(other)` arm) no longer occurs exactly once
- **Files:** `contracts/messages/scenarios/` — inferred: new `llm.messages.DecodeStream` scenarios for the two failures and the two streams after text
- **Files:** `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json` — inferred: list the scenarios, regenerate the suite, raise the floor from 1068
- **Symbols:** `Header::outcome`, `stop_reason`, `Header::attach`, `StreamDecoder::finish`, `decode_message` — cited
- **Also likely:** `crates/llm-messages/src/ingress.rs` — inferred: `encode_ingress_message` (:64) and `reason` (:700) would still send an `Incomplete` answer without content, which the decoder would then refuse; the encoder's rule is to send only what reads back as the same outcome
- **Also likely:** `crates/llm-messages/tests/retry_class.rs` — inferred: the retry class of the new failure through `MessagesClient`
- **Also likely:** `spec/domains/messages.yaml` — inferred: only the rule comment above `llm.messages.DecodeStream` (:233-240); `error_code` and `usage_json` already exist
- **Not expected:** `checks/conformance/src/messages.rs`, `crates/llm-core/src/turn.rs`, `crates/llm-core/src/error.rs` — inferred: `report_failure` (:337-351) already writes a failed decode's code and usage; the error codes and `with_retriable` exist
- **Documents:** `docs/messages.md` "What the decoder requires of a stream" (:150-154) and "Reasons" (:452); `docs/design.md` "Provider failures retried before output" (:447-474) — cited
- **Documents:** `CHANGELOG.md` Unreleased; `AGENTS.md` invariants rows (retry classes, scenario floor); `docs/implementation-status.md` then `task docs-generate` — inferred. No website page states the rule
- **Confidence:** high for the crate and `decode.rs`; medium for contracts and documents; low for `ingress.rs`
- **Would collide with:** any unit touching `crates/llm-messages/src/decode.rs`, `crates/llm-messages/tests/streaming.rs` or the Messages falsification record, and the shared files `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json`, `CHANGELOG.md`, `AGENTS.md`
- **Safety fact:** every `Err` out of `Header::outcome` already passes `Header::attach` (`decode.rs:216-217`, `:805`), which sets dispatch `accepted` and attaches the usage read so far with `final_usage` false; that one function finishes a stream, a complete message and Bedrock InvokeModel (`crates/llm-bedrock/src/invoke.rs:147`, `:350`) — unproven
