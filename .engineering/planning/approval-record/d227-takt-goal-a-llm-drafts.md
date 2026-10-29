---
format: aep.planning-md/3
id: approval-record:d227-takt-goal-a-llm-drafts
kind: approval-record
status: approved
title: Two llm drafts made runnable for takt goal A under orchestrator decision D227
relations:
- decides: story:http-transport-load-sensitivity
- decides: story:chat-counts-every-output-delta
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-10-29T21:34:41Z", actor: "human:timo", revision: 2}
---
## Decision

`story:http-transport-load-sensitivity` and `story:chat-counts-every-output-delta` are made
runnable and accepted (`draft` to `proposed`) for takt's in-cluster goal runs (goal A). They are
not implemented in wave 2026-10-29-w23; the change lands with that wave's closing store commit.

## Instruction

Session `b10x-chef-orchestrator`, decision D227 (2026-10-29), under the owner's standing grant,
quoted:

> takt's in-cluster goal runs (goal A, Timo 2026-10-29) will implement two llm drafts, not your
> D221 stories. Make both runnable and move them to proposed. Do not implement them.

For `story:http-transport-load-sensitivity`: "acceptance with a load level someone can check; the
line \"the fix stays in the test\"."

For `story:chat-counts-every-output-delta`: "a Scope section; file:line references refreshed
against main; the choice for `refusal` and `reasoning` deltas, recorded as decided by me (D227).
Recommended default: count them in usage like other output deltas and carry them through as their
own kinds. Refuse one only if the wire gives no way to tell it apart. Write down what you found."

## What was written

- `story:http-transport-load-sensitivity`: two checkable load levels (one CPU shared with 4
  nice-19 workers, 10 of 10; the test's bound lowered to 1 ms in a scratch copy, 10 of 10), the
  fix-stays-in-the-test line in the acceptance, and the scope refreshed at `bb3bda33`.
- `story:chat-counts-every-output-delta`: references refreshed at `bb3bda33`, the D227 choice with
  what was found (a new `StreamEvent` kind for `refusal`; `reasoning` as `ReasoningDelta`; a delta
  whose two reasoning keys differ is the one refused), a Scope section.
