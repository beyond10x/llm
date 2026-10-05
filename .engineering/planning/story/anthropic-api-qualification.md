---
format: aep.planning-md/3
id: story:anthropic-api-qualification
kind: story
status: draft
title: The Anthropic API route is qualified with a live API-key turn (deferred)
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
revision: 1
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
