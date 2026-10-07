---
format: aep.planning-md/3
id: story:responses-overload
kind: story
status: active
title: Responses overloads are retryable availability before output
relations:
- informed_by: story:parity-retry-classes
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: contracts/responses
- confidence: cited
  path: crates/llm-responses
- confidence: cited
  path: docs/responses.md
- confidence: cited
  path: spec/domains/responses.yaml
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-07T12:53:35Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-07T12:53:35Z", actor: "human:timo", revision: 4}
---
## Outcome

A benign clock query surfaced a provider overload as a request refusal. The Responses adapter
recognizes only the exact `server_is_overloaded` code as temporary availability; it does not
retry automatically and never offers a replay after any prior output payload.

## Evidence and acceptance

A live Loom reproduction on 2026-10-07 reached the admitted clock action, then the top-level
Responses SSE error carried `server_is_overloaded`; existing llm returned `Refused`.
The original diagnostic stays in the caller's archived qualification evidence, not source.

ESS scenario `provider-overload-is-availability` observes the actual decoder's class, accepted
dispatch, unknown terminal usage and fixed diagnostic. Real ResponsesClient loopback tests must
be red on the old implementation and green for lifecycle-only prelude, visible text and silent
opening output item, nested response failure, actual refusal and unknown code. No provider text
may reach errors, and no client may silently resend a request.

## Scope

Responses classification and the client's existing prior-output retry guard; specification,
scenario projections, regression tests and capability documentation. Caller retry policy and
metrics belong to Loom and are implemented in its own change.
