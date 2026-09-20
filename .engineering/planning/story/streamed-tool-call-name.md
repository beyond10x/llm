---
format: aep.planning-md/1
id: story:streamed-tool-call-name
kind: story
status: draft
title: A streamed tool call carries the name the provider announced
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-chat
- confidence: inferred
  path: crates/llm-core
- confidence: inferred
  path: crates/llm-messages
- confidence: inferred
  path: crates/llm-responses
revision: 2
---
## Context

A streamed tool call becomes visible before its arguments do. Every protocol in scope announces the
tool's name once, in the fragment that opens the call, and never repeats it in the argument deltas
that follow.

The neutral interface cannot carry that announcement. `StreamEvent` has `ToolArgumentsDelta`, which
holds a `CallId` and a fragment of text, and nothing that carries the name the provider sent with
the opening fragment. A consumer re-emitting the stream therefore cannot name the call it is
relaying, and a gateway translating between protocols cannot produce a valid opening fragment for
the protocol it is writing to.

Found by `story:chat-projection` while implementing the Chat Completions decoder, which meets the
gap first because that protocol's first tool chunk is exactly the one carrying the name. It worked
around the gap rather than inventing a name: calls are emitted once, complete, in the terminal
chunk, instead of streamed under a name the interface could not carry. That workaround is correct
and is the reason the story is not blocked, but it also means a caller cannot see a tool call until
the turn ends, which is a behaviour difference from the provider it is projecting.

The core crates were frozen for wave 1, so the change was proposed rather than made. The proposed
shape is one added variant:

```rust
ToolCallStarted { call_id: CallId, name: ToolName },
```

The proposal is retained at `docs/plan/wave-1-core-request.patch` and cites
`crates/llm-core/src/port.rs`.

## Acceptance

A streamed tool call carries its name at the point the provider announced it, and a consumer that
re-emits the stream names the call without inventing or deferring it. The Chat, Responses and
Messages projections each produce the new event from their own opening fragment, and the existing
terminal-chunk behaviour no longer stands in for it.

## Evidence

`story:chat-projection` implementation report, 2026-09-21, and the patch it wrote rather than
applied. `crates/llm-core/src/port.rs` holds the enum. The three projection crates hold the
decoders that would produce the event.

## Verification

The envelope version moves, because this is a change to a published contract and the repository
refuses a silent one. Each projection's conformance scenarios assert the event's presence and its
ordering relative to the argument deltas that follow it, and a deliberate defect that drops the
announcement fails a named scenario. No paid provider call runs in the ordinary gate.

## Scope

- inferred: `crates/llm-core` — the enum and its envelope version.
- inferred: `crates/llm-chat`, `crates/llm-responses`, `crates/llm-messages` — each decoder emits it.
- inferred: `contracts` — scenarios asserting the announcement and its ordering.

This lands after wave 1, because it changes a crate every projection depends on and three of them
were being written at once. It is the coupling the wave's freeze existed to prevent, surfaced as a
proposal instead of a conflict.
