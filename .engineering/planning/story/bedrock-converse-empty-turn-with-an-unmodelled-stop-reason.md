---
format: aep.planning-md/3
id: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
kind: story
status: implemented
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
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/bedrock/scenarios
- confidence: inferred
  path: contracts/ess-inputs.yaml
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
  path: crates/llm-bedrock/tests/data/README.md
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
- confidence: inferred
  path: website/docs/guides/call-a-model-on-bedrock.md
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T07:45:27Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T07:45:27Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T12:21:41Z", actor: "human:operator", revision: 9, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
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

Read by the coordinator from `git diff --stat` of the integration branch against the wave's opening
commit (`7ddbd1d6d`) at the close of wave 2026-11-25-converse-empty-turn: 28 files. The section
before this one was derived by `story-scoper` on 2026-11-25; it named every path below except the
Bedrock guide, the test data README and `contracts/ess-inputs.yaml`.

| Path | Scoped | What the unit did |
| --- | --- | --- |
| `crates/llm-bedrock/src/converse.rs` | cited | `Header::outcome` takes whether the answer held a content block and fails an answer that stopped for an unmodelled reason with no content block and no kept event; `stopped_without_content` holds the rows; `StreamDecoder::read_a_content_block` |
| `crates/llm-bedrock/src/client.rs` | inferred | `whole` drops the retriable mark of the Converse failure too; the struct doc of `BedrockClient` names the one retriable failure of a stream that ended |
| `crates/llm-bedrock/tests/converse.rs`, `tests/client.rs` | inferred | the rows streamed and whole, usage, the fixed text, nothing shown, an empty text block, a start that names `text`, the advisory of a failed turn, both wires read alike; no new test binary |
| `crates/llm-bedrock/tests/data/frames`, `tests/data/README.md` | inferred (the directory); the README not scoped | three whole Converse answers written by hand (`synthetic-converse_*.json`), named in the README |
| `crates/llm-models/tests/relayed_failure.rs` | inferred | one case: the Converse client asks once a turn and routing takes it under no catalog target. Two cases that put a Converse client under routing's loop through a test adapter were written and removed (correction 1) |
| `spec/domains/bedrock.yaml` | inferred | the "Failures" comment above `llm.bedrock.TurnObservation` states the rule |
| `contracts/bedrock/scenarios`, `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json` | inferred; `ess-inputs.yaml` not scoped | ten scenarios `turn-a-converse-stream-*stopped-*` and `turn-a-whole-converse-answer-stopped-*`; the suite regenerated; the floor 1080 → 1090 |
| `docs/design.md` | cited | "Provider failures retried before output" names the Converse case and its one difference from the Messages wire |
| `docs/bedrock.md` | inferred | "Retry classes" rewritten for who retries; the section "A Converse answer that stops without content" with its table of tests |
| `website/docs/guides/call-a-model-on-bedrock.md` | not scoped | the failures and the no-retries bullets say what the client and routing do |
| `AGENTS.md` | cited | the provider-failure row, the Bedrock retry row and the conformance floor row of the invariants table |
| `CHANGELOG.md` | not scoped | six lines under `### Fixed`, the first marked breaking |

Not touched: `crates/llm-messages`, `crates/llm-routing`, `crates/llm-cli`, `contracts/schema/`,
`README.md` and `docs/implementation-status.md`.
