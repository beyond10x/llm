---
format: aep.planning-md/3
id: story:messages-empty-turn-with-an-unmodelled-stop-reason
kind: story
status: implemented
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
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:00Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:00Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T01:56:23Z", actor: "human:operator", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
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

Read by the coordinator from `git diff --stat` of the unit branch `impl/messages-empty-turn` (head
`fa7f048ea`) against its base `8938c6085`, at the close of wave 2026-11-25-messages-empty-turn. The
`story-scoper`'s section of 2026-11-25 (read at `87bc16f16`) named the crate, `decode.rs` and
`Header::outcome`; every `inferred` line of the frontmatter held except
`docs/implementation-status.md`, whose one changed row is about the usage extension, not the rule.

| Path | Scoped | What the unit did |
| --- | --- | --- |
| `crates/llm-messages/src/decode.rs` | cited | `stopped_without_content` in `Header::outcome`: the three rows, final usage, dispatch `accepted`; usage extensions held while a turn may stop without content; `StreamDecoder::fail` and `StreamDecoder::end` |
| `crates/llm-messages/tests/streaming.rs` | cited | the rule's cases on a stream and a whole message, the held-cost cases, the adversary's (`adversary_w30_*`, `adversary_w30b_*`) |
| `docs/verification/messages-falsification.json` | cited | the new fixed texts under `unaimed`, counts, notes; no recorded mutation changed |
| `docs/messages.md`, `docs/design.md` | cited | the rule, what "no content" means, when a held cost is shown or dropped; the retry rule's amended count of output |
| `contracts/messages/scenarios` | inferred | 7 scenarios |
| `contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/baseline.json` | inferred | generated with ess 0.87.0; the floor is 1075 |
| `crates/llm-messages/src/ingress.rs` | inferred | the encoder refuses an `Incomplete` outcome with no items as `unsupported` |
| `crates/llm-messages/tests/retry_class.rs` | inferred | the retry class through `MessagesClient`, also on a route that reports a cost |
| `spec/domains/messages.yaml` | inferred | the rule comment above `llm.messages.DecodeStream` |
| `CHANGELOG.md`, `AGENTS.md` | inferred | `### Changed` and `### Fixed` under Unreleased; the retry row and the floor row |
| `docs/implementation-status.md`, `website/data/status.json` | inferred | one row on when a usage extension is shown; the generated copy |
| `crates/llm-messages/src/client.rs` | no | the client ends a stream with `fail` and `end` |
| `crates/llm-messages/tests/ingress.rs`, `tests/usage_reader.rs`, `tests/wire_names.rs` | no | the encoder rule, the reader against the decoder on the failure, the two reason names as confirmed wire names |
| `crates/llm-bedrock/src/client.rs`, `src/invoke.rs`, `tests/client.rs` | no | a whole `InvokeModel` answer is final; the streamed path shows held costs before a final cut |
| `crates/llm-models/tests/relayed_failure.rs` | no | the bounded retry through client and routing (4 requests), the priced failed attempt |
| `docs/bedrock.md` | no | the retry table's rows for the streamed and the whole answer |

Not touched, as scoped: `checks/conformance/src/messages.rs`, `crates/llm-core/`. Not covered:
Bedrock Converse, which has its own copy of the reason mapping
(`story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`).
