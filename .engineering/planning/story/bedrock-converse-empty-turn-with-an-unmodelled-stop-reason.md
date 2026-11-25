---
format: aep.planning-md/3
id: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
kind: story
status: active
title: A Bedrock Converse turn that stops for an unmodelled reason without content is a classified failure
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w30-messages-empty-turn-pass-1
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: AGENTS.md
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/bedrock/scenarios
- confidence: inferred
  path: contracts/suite.json
- confidence: inferred
  path: crates/llm-bedrock/src/client.rs
- confidence: cited
  path: crates/llm-bedrock/src/converse.rs
- confidence: inferred
  path: crates/llm-bedrock/tests/client.rs
- confidence: inferred
  path: crates/llm-bedrock/tests/converse.rs
- confidence: inferred
  path: crates/llm-bedrock/tests/data/frames
- confidence: inferred
  path: crates/llm-models/tests/relayed_failure.rs
- confidence: inferred
  path: docs/bedrock.md
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: spec/domains/bedrock.yaml
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T07:45:27Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T07:45:27Z", actor: "human:operator", revision: 3}
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

## Scope

Derived 2026-11-25 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-bedrock` — cited
- **Files:** `crates/llm-bedrock/src/converse.rs:466-510` — cited
- **Symbols:** `Header::outcome` (466-492, builds the outcome, calls `stop_reason` at 473), `stop_reason` (498-510, doc comment 495-497), `Header::attach` (452-464), `Header::observation` (441-449), `StreamDecoder::finish` (1206-1224), `read_answer` (1310-1354), `decode_response` (1292-1308) — cited
- **Also likely:** `crates/llm-bedrock/src/client.rs:483-487` — inferred; `whole` strips `retriable` from every whole-answer error and its comment names only the Messages case
- **Tests today:** `crates/llm-bedrock/tests/converse.rs::every_terminal_reason_is_carried_and_none_reads_as_completion` (897-919; streamed only, always after one text block; `max_tokens`, `guardrail_intervened`, `content_filtered`; helpers `event` 860, `synthetic_answer` 881). No test has an unmodelled reason with no item, and none on a whole Converse answer — cited
- **Tests land in:** `crates/llm-bedrock/tests/converse.rs` and `crates/llm-bedrock/tests/client.rs` (the InvokeModel sibling cases are at `client.rs:1030-1332`, table `STOPS_WITHOUT_CONTENT` 1040-1065) — inferred. `crates/llm-bedrock/tests/` holds 18 top-level `.rs` files, each its own test binary, and no `tests/main.rs`; `process_credentials` is a `[[test]]` with `harness = false`. A new file would add a binary
- **Also likely:** `crates/llm-models/tests/relayed_failure.rs` — inferred; "bounded by the retry policy" is held there for Messages (line 265) and `BedrockClient` makes one attempt (`docs/bedrock.md:272`)
- **Specification first:** `spec/domains/bedrock.yaml:122-131` (the "Failures" comment above `llm.bedrock.TurnObservation`, fields `error_code` 136, `retriable` 139, `stop_reason` 164) — inferred as the file that changes first. No file under `spec/` or `contracts/bedrock/` names a Converse stop reason today; line 117 says only that InvokeModel's stop reasons are the Messages wire's — cited
- **Scenarios:** new `contracts/bedrock/scenarios/turn-*.yaml`, `contracts/suite.json`, `contracts/baseline.json` (`AGENTS.md:80`, at least 1080) — inferred. `llm.bedrock.RunTurn` (`spec/domains/bedrock.yaml:281-314`) scripts the stream case as is: `keep_frames: 1` plus `frames_json` events (adapter `checks/conformance/src/bedrock.rs:663-690`). The whole case needs a new `.json` answer under `crates/llm-bedrock/tests/data/frames/` (adapter 711-717)
- **Documents:** `docs/design.md:447-501` ("Provider failures retried before output"; Messages paragraph 476-501, whole-answer sentence 499-501) — cited. `AGENTS.md:76` (the Bedrock retry row) and `AGENTS.md:65` (the provider-failure row that names the Messages case) — cited. `docs/bedrock.md:270-299` (Retry classes; rows 286-287 name only InvokeModel) — inferred
- **Confidence:** high — the story names the file and the function, and both were read
- **Would collide with:** any unit touching `crates/llm-bedrock/src/converse.rs`, the two test files above, `spec/domains/bedrock.yaml`, the invariants table of `AGENTS.md`, the retry section of `docs/design.md`, or adding any scenario (`contracts/suite.json`, `contracts/baseline.json`)
- **Safety fact:** a Converse answer that decodes to no item showed the caller no text, reasoning or tool event, so failing it retracts nothing: an empty text fragment emits no event (`converse.rs:749-751`) and leaves no item (1146), every other closed block leaves an item (1147-1161) — unproven
