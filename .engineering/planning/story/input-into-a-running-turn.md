---
format: aep.planning-md/3
id: story:input-into-a-running-turn
kind: story
status: active
title: Input reaches a running turn, or llm records that it does not and names the supported pattern
summary: Model::turn takes a fixed TurnRequest; no input reaches a turn in progress, and no protocol in scope can carry it
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:capabilities-declare-streaming
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: crates/llm-docs/examples
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: website/docs/guides
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T15:37:08Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-20T15:37:09Z", actor: "human:timo", revision: 5}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100). A speech graph can receive new user input
while the model is still answering, and wants that input to reach the running turn.

At tag 0.7.0 nothing can reach a running turn: `Model::turn(&self, request: &TurnRequest, sink,
cancel)` (`crates/llm-core/src/port.rs:43-52`) takes a fixed request, and the only signal a caller
can send during the turn is `cancel`. inference works around it by cancelling or letting the turn
finish and sending the new input as the next turn's user text.

None of the three protocols this workspace speaks (Responses, Messages, Chat Completions, as
`docs/design.md` scopes them) accepts input into a response that is still streaming. A binding that
could needs a bidirectional protocol, which llm does not have.

## Outcome

Either a caller can send input into a running turn on a binding whose protocol accepts it, or
`docs/design.md` records that llm does not offer this and names the supported pattern (cancel, then
a new turn carrying the input), so the next consumer does not look for it.

## Acceptance

- `docs/design.md` states which of the two holds and why.
- If input into a running turn is offered: a protocol that carries it, a port method or request
  stream that delivers it, a declaration in `Capabilities` that says which bindings accept it, and a
  test that sends input during output and observes it in the same turn.
- If it is not: a guide program in `crates/llm-docs/examples/` shows cancel-then-continue, keeping
  the text already shown as assistant text of the new turn.

## Depends on

The input-during-output declaration in `Capabilities` (story capabilities-declare-streaming).

## Scope (inferred)

`docs/design.md`; for the offered branch also `crates/llm-core/src/port.rs` and a new protocol
crate; for the other branch `crates/llm-docs/examples/` and a guide page.

## Design decisions

Approved with wave 2026-10-20-w11 by session `b10x-chef-orchestrator` under the owner's grant:
option B, not offered. None of the three protocols in scope carries input into a response that is
still streaming; `docs/design.md` says so and names the supported pattern (cancel, then a new turn
carrying the input), and a guide example in `crates/llm-docs/examples/` shows it, keeping what the
cancelled turn already streamed.
