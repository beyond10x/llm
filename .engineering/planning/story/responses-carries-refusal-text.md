---
format: aep.planning-md/3
id: story:responses-carries-refusal-text
kind: story
status: draft
title: A Responses binding carries refusal text both ways
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- informed_by: review-result:adversary-w24-chat-deltas-pass-1
revision: 1
---
## Context

Wave 2026-10-30-w24 (`story:chat-counts-every-output-delta`) added `Item::AssistantRefusal` and
`StreamEvent::RefusalDelta`. Only the Chat wire carries them; the Responses projection and its
ingress encoder refuse them by name, and routing selection rejects a non-Chat target for a
conversation that carries refusal text (`Rejection::RefusalText`).

`review-result:adversary-w24-chat-deltas-pass-1` (finding at `docs/chat.md:94`) points out that the
Responses wire likely has a `refusal` content part on assistant output messages. Inferred from the
public API reference, not checked against a server or a recorded fixture in this repository.

## Outcome

A Responses binding carries refusal text both ways, so a conversation holding a refusal can be
routed to a Responses target.

## Acceptance

- A Responses stream whose output message holds a `refusal` content part reaches the caller as
  `StreamEvent::RefusalDelta` and `Item::AssistantRefusal`, never as assistant text.
- An `Item::AssistantRefusal` in a request is projected as that content part, and the Responses
  ingress encoder writes it.
- Selection no longer rejects a Responses target for refusal text.
