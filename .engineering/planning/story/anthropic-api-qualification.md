---
format: aep.planning-md/3
id: story:anthropic-api-qualification
kind: story
status: draft
title: The Anthropic API route is qualified with a live API-key turn (deferred)
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: .engineering/planning/credential-blocker/anthropic-live-qualification.md
- confidence: cited
  path: .engineering/planning/story/anthropic-api-qualification.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: crates/llm-credentials/Cargo.toml
- confidence: inferred
  path: crates/llm-credentials/examples
- confidence: inferred
  path: crates/llm-credentials/examples/live_subscription_turn/turn.rs
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: docs/live-qualification.md
revision: 4
---
## Outcome

A live report demonstrates one supported model turn over the Anthropic API with an API key,
identifying the endpoint and auth contract, verifying protocol behaviour and credential rotation,
and naming unsupported cases.

## Why

Split from story:anthropic-access. Operator, 2026-10-05: "i dont want to run with anthropic api key
for now, but with claude sub". Drafted so the API-route qualification is not lost.

## Acceptance

The report is recorded as evidence here; the key resolves through the secrets library.

## Scope

Scoped 2026-10-08 at 560f044c; confidence medium (the story names no path; read off story:anthropic-access).

- Primary surface: `crates/llm-credentials/examples` (inferred), a sibling of `live_subscription_turn` or an API-key mode on it; `turn.rs:60` lists `api-key-route` as not exercised.
- No library change expected: `AuthKind::ApiKey` with `x-api-key` is already presented (`crates/llm-providers/src/auth.rs:82`) and fixture-checked (`crates/llm-messages/tests/client.rs:140`); unproven against a live endpoint.
- Documents: `docs/live-qualification.md`, `docs/implementation-status.md` (`anthropic-access` row), the credential blocker `anthropic-live-qualification`.
- Collides with story:anthropic-access's live-evidence surface (`live_subscription_turn`, `docs/live-qualification.md`).
- Open: whether the live Messages decoder accepts an API-key response (the subscription route's first live turn was refused on an undeclared field); no default secret name for the API key exists yet.
