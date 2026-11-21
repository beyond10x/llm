---
format: aep.planning-md/3
id: story:client-cache-marks
kind: story
status: implemented
title: A Messages client's cache marks reach Messages and InvokeModel where the client placed them
summary: Hints::cache_marks carries the placement; the encoders send exactly those marks and add none
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-bedrock/tests/client_cache_marks.rs
- confidence: cited
  path: crates/llm-core/src/controls.rs
- confidence: cited
  path: crates/llm-core/src/turn.rs
- confidence: cited
  path: crates/llm-messages/src/codec.rs
- confidence: cited
  path: crates/llm-messages/tests/client_request_fields.rs
- confidence: cited
  path: docs/bedrock.md
- confidence: cited
  path: docs/design.md
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: spec/domains/inference.yaml
- confidence: cited
  path: spec/domains/messages.yaml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-20T23:53:17Z", actor: "agent:ai-agent-platform", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-20T23:53:18Z", actor: "agent:ai-agent-platform", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-21T00:29:12Z", actor: "agent:ai-agent-platform", revision: 5, decided_on: {"recorded":{"test_result":1},"asserted":{"test_result":1}}}
---
## Source

The operator, 2026-11-21: "you do need to make sure that we're not dropping any behavior. So if
LLM gateway is having that weird behavior of dropping the effort, make sure this isn't happening.
So we need to fix the LLM gateway then. You can just do that in this session."

A deployment that serves Claude on Bedrock through llm-gateway measured it (gateway 0.22.0, llm
0.20.0, a loopback Bedrock fixture): every `InvokeModel` body carried
`cache_control: {"type":"ephemeral"}` on the last message's first content block, also when the
client sent no mark; a client's mark on its system block arrived beside a second one on the tail.
The marks are this repository's: `crates/llm-messages/src/codec.rs` `project` calls
`mark_rolling_breakpoint` and marks the joined system block, and ingress keeps only the marks'
lifetime (`docs/messages.md`: "the caller's mark positions are not reproduced"). A cache write is
priced above an uncached read, so a mark the client did not send changes what it pays. The
gateway cannot fix it alone: the body is encoded and signed inside `llm_bedrock`.

## Outcome

`Hints::cache_marks` (`CacheMarks`: `instructions`, `items`, `tools` by index into the turn)
records where a client placed its marks. The Messages ingress fills it whenever a request
carries at least one mark; the Messages and `InvokeModel` encoders then send exactly those marks,
each with the turn's prompt-cache lifetime, and none of their own. Empty, no mark is sent; absent
(a request with no mark, a chat or responses client), the projection keeps its own breakpoints.
A caller that wants no automatic mark sets it empty.

## Acceptance

1. A request whose only mark is on its system block reaches `InvokeModel` with exactly that mark.
2. Marks on content blocks and a tool arrive on the same blocks with their lifetime; a later
   `system` entry spliced in as user text does not move a mark after it.
3. A request with no mark keeps the projection's breakpoints; an empty `cache_marks` sends none.
4. A mark index outside the turn, or named twice, is refused `invalid` by `validate`.
5. `docs/messages.md`, `docs/design.md`, `docs/bedrock.md`, the `llm.inference` and
   `llm.messages` domains and `CHANGELOG.md` say so.
