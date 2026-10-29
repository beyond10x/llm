---
format: aep.planning-md/3
id: review-result:adversary-w21-unpublished-calls-pass-1
kind: review-result
status: active
title: 'Adversary pass 1: story:unpublished-tool-call-reaches-an-opted-in-caller'
relations:
- reviews: story:unpublished-tool-call-reaches-an-opted-in-caller
revision: 1
---
needs-change, resolved on the wave branch

Adversary pass 1 on `story:unpublished-tool-call-reaches-an-opted-in-caller` (commit 320bea5a, wave
2026-10-29-w21, D202). Twenty-four cases in `adversary_w21_unpublished.rs` across llm-core,
llm-chat, llm-messages, llm-responses, llm-bedrock and llm-routing: the control on every wire is
local (bodies byte for byte unchanged), an opted-in caller's refusal answer encodes on each wire,
every other validation still refuses. One blocker, fixed: Bedrock Converse returned a name it
cannot carry back, so it now refuses that turn as it did before the control.

```findings
[
{"file":"crates/llm-bedrock/src/converse.rs","line":409,"category":"acceptance","severity":"blocker","verdict":"NEEDS-CHANGE","origin":"introduced","message":"Converse handed an opted-in caller an unpublished name outside [A-Za-z0-9_-] (repo_browser.open_file) that the next Converse turn cannot encode; fixed on the wave branch: such a turn is refused as before the control, case rewritten citing the story"},
{"file":"crates/llm-core/tests/unpublished_calls.rs","line":174,"category":"coverage","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"the core next-turn case checks validate only; the wire answer is covered by the adversary files in llm-chat, llm-messages, llm-responses, llm-bedrock and llm-routing"},
{"file":"crates/llm-core/src/turn.rs","line":384,"category":"acceptance","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"tool_choice required with only unpublished calls passes for an opted-in caller; accepted as consistent with the opt-in"}
]
```
