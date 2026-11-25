---
format: aep.planning-md/3
id: story:tool-arguments-with-a-repeated-key
kind: story
status: draft
title: Tool-call arguments that repeat a key do not reach a caller with one reading chosen
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w27-rubric-judge-pass-1
- decomposes: epic:inference
revision: 1
---
## Context

`story:classifier-takes-only-a-whole-answer` makes the classifier abstain on an answer that gives a
key twice, on its JSON-schema path, where the answer is text and `llm-decide` reads its bytes.

On the forced-tool path the answer is the arguments of a tool call. `ToolCall.arguments` is an
already-parsed `serde_json::Value` (`crates/llm-core/src/item.rs`), and the three protocol decoders
keep the last value of a repeated key when they parse it (`crates/llm-messages/src/decode.rs`,
`crates/llm-responses/src/stream.rs`, `crates/llm-chat/src/incoming.rs`; read by the `story-scoper`
of 2026-11-25 at `87bc16f16`). So a model that answers `{"label":"a","label":"b"}` as tool arguments
is read as `b`, and no caller of the library can see that the key came twice. Whether `llm-bedrock`
parses tool arguments the same way is not established.

The defect is in the protocol crates and was there before wave 2026-11-25-classifier-whole-answer.

## Outcome

Tool-call arguments that give a key twice, at any depth, do not reach a caller as a call with one
reading chosen for it. A classifier on the forced-tool path abstains `OffLabel` on such an answer.

## Open

What a protocol decoder does with such arguments: refuse the turn as a protocol error, or carry the
call marked so that the caller decides. The first changes every caller of tools, not only the
classifier.

## Acceptance

- For Messages, Responses, Chat Completions and each Bedrock path that carries a tool call, a
  streamed and a whole answer whose tool arguments repeat a key has a test that fails before the
  change.
- `llm-decide` has a forced-tool test for the repeated `label` key that abstains `OffLabel`.
- The strict reader is one function; `crates/llm-decide/src/answer.rs` and
  `crates/llm-openai-decisions/src/strict.rs` do not each keep a copy.

## Verification

Targeted: the named test targets of the protocol crates and of `llm-decide`, and the conformance
check. No live call.
