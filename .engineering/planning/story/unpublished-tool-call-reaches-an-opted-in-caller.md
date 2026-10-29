---
format: aep.planning-md/3
id: story:unpublished-tool-call-reaches-an-opted-in-caller
kind: story
status: active
title: A call to an unpublished tool reaches a caller that asked for it
summary: controls.return_unpublished_calls returns a well-formed unpublished call instead of refusing the turn; the default still refuses (D202)
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T11:27:34Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-29T11:27:35Z", actor: "human:timo", revision: 3}
---
## Context

Orchestrator item D202, from Harness wave D200 (beyond10x/harness pull request 59, merge
`3b88c4d`; Harness draft `story:verbs-survives-a-bare-entry-call`). Three live `--surface verbs`
runs on `claude-haiku-4-5-20251001` through llm 0.16.0 ended on turn 2 with `Protocol: model
output contains a duplicate call or unpublished tool`: the model called `find` and `file_read`,
entry names the verbs surface does not publish, and the provider accepted them.

`TurnOutcome::validate_for` (`crates/llm-core/src/turn.rs`) refuses the whole turn for a call to a
tool the request did not publish. Harness invariant 9 says an unpublished tool is an outcome the
model learns about, never the end of the run, and its loop already answers such a call as a
refusal; the check in llm makes that answer unreachable.

## Decision

The refusal is a documented contract (scenario
`model-output-calling-an-unpublished-tool-is-refused`, `docs/chat.md`, `llm-tool-call`'s
`ModelError::WrongTool` mapping), so its default does not change. A caller opts in per turn with a
new control, `TurnRequest.controls.return_unpublished_calls` (absent or false: refuse, as today).
With it set, a well-formed call whose name the request did not publish reaches the caller as an
ordinary `Item::ToolCall`. Everything else `validate_for` checks still refuses: a duplicate call id,
a call other than a named tool choice, oversized arguments, caller-owned content, foreign or
unattributed opaque state, missing terminal evidence and a stop reason that contradicts the calls.

The control is local to outcome validation: no egress wire sends it and no ingress decoder sets it.
A turn carrying it is written as `llm.turn/4`, as every turn with controls is.

## Acceptance

- `crates/llm-core/tests/unpublished_calls.rs`: a turn shaped like the live record (a call to
  `find` beside published tools) is refused without the control and returned with it; with it, a
  duplicate call id, a call other than a named tool choice and oversized arguments still refuse.
- `crates/llm-messages/tests/streaming.rs::a_tool_call_the_request_never_published_reaches_a_caller_that_asked_for_it`
  (the decoder path the live run took).
- Scenarios `contracts/inference/scenarios/model-output-calling-an-unpublished-tool-reaches-a-caller-that-asked-for-it.yaml`
  and `model-output-repeating-a-call-id-is-refused-even-for-a-caller-taking-unpublished-calls.yaml`;
  `model-output-calling-an-unpublished-tool-is-refused` unchanged.
- Bedrock Converse returns an unpublished call only when its name fits the tool-name class a next
  Converse turn can carry; otherwise the turn is refused with a protocol error, as without the
  control (`crates/llm-bedrock/tests/adversary_w21_unpublished.rs::an_unpublished_call_converse_returned_can_be_answered_on_converse`).
