---
format: aep.planning-md/3
id: story:responses-websocket-steering
kind: story
status: draft
title: A Responses binding that supports it carries input into a running turn (WebSocket steering)
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 1
---
## Context

Found by the adversary of `story:input-into-a-running-turn` in wave 2026-10-20-w11
(`review-result:adversary-w11-running-turn-pass-1`). OpenAI documents mid-turn steering for the
Responses API: "Mid-turn steering is available with the GPT-6 model family over a WebSocket
connection to the Responses API. GPT-5.6 and earlier models do not support steering." A client
starts a response with `response.create`, then sends `response.steer` on the same connection with
that response's id as `previous_response_id`; the API acknowledges with `response.steer.accepted`,
ends the running response and continues with the new input
(developers.openai.com/api/docs/guides/steering, fetched 2026-10-20).

llm's Responses client speaks HTTP with server-sent events and does not implement WebSocket mode, so
`story:input-into-a-running-turn` recorded that llm does not offer input into a running turn and
named the cancel-then-continue pattern. Anthropic Messages and Chat Completions document no such
feature. The inference repository (Atlas ADR 0100) asked for input into a running turn for its
speech graph.

## Outcome

On a Responses binding whose model and route support it, a caller can send input into a running
turn and observe it in the same turn, and `Capabilities::input_during_output` declares exactly the
bindings that accept it.

## Acceptance

- A Responses WebSocket transport, or a documented reason not to build one, decided first in
  `docs/design.md`.
- A port method or request stream that delivers input into a running turn, and its effect on the
  turn's outcome (the steered response ends `incomplete`, reason `steered`, and continues).
- `Capabilities::input_during_output` is `true` only for a binding that can carry it, and
  `validate_protocol` refuses it elsewhere.
- A test against a pinned WebSocket fixture sends input during output and observes it in the same
  turn; no paid provider call.
