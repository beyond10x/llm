---
format: aep.planning-md/3
id: review-result:adversary-w25-structured-output-pass-1
kind: review-result
status: active
title: Wave 2026-11-04-w25 adversary, llm story:structured-output, pass 1
relations:
- reviews: story:structured-output
revision: 1
---
```
unit: beyond10x/llm story:structured-output, diff 86eb40978..b980920e7, cases commit cfbd4c607
verdict: NEEDS-CHANGE (3 red cases, 2 findings)
cases: executed 30→41, red 3
origin: introduced 2 / pre-existing 0 / undecided 0
needs-coordinator: whether `strict: true` is refused or honoured on Messages, InvokeModel and Converse
```

Red: `strict: true` is encoded with no member and no refusal on Messages, Bedrock InvokeModel and
Converse, where the acceptance said a member the wire has no place for is refused by name.

Coordinator's decision: both providers document that the answer is always constrained to the schema
(Anthropic structured outputs; Bedrock "strict schema compliance" through constrained decoding), so
`strict: true` is honoured by construction and `strict: false` stays refused. The acceptance is
amended to say so, and the three cases are rewritten to it with the story id in their messages. The
same AWS page shows `structure.jsonSchema.name` on Converse, which the unit did not send; that is
fixed in the wave.

Held: a TurnDocument without the format keeps its bytes and version; name bounds on core,
Responses, Chat and Converse; Responses and Chat round trips; unknown keys refused by path;
`json_object` refused by type; the format beside `output_config.effort`; additive for consumers (the
one consumer `Controls` literal uses `..Default::default()`); routing cannot drop the format.

Out of scope: selection does not reject a nameless format for Responses or Chat ahead of dispatch.

```findings
[
  {"file":"crates/llm-messages/src/codec.rs","line":714,"category":"acceptance","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"a format with strict true is sent on Messages and Bedrock InvokeModel with the flag left out, where the acceptance required a refusal by name"},
  {"file":"crates/llm-bedrock/src/converse.rs","line":171,"category":"acceptance","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"a format with strict true is sent on Bedrock Converse with the flag left out, where the acceptance required a refusal by name"}
]
```
