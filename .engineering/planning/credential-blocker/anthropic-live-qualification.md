---
format: aep.planning-md/3
id: credential-blocker:anthropic-live-qualification
kind: credential-blocker
status: open
title: The live API and subscription runs need the operator's credentials
relations:
- blocks: story:anthropic-access
revision: 2
---
## What is missing

The story's acceptance asks for a live qualification report per route: one supported model turn
over the operator's own subscription token, naming the endpoint and auth contract it used (the
API-key run is deferred to story:anthropic-api-qualification, operator 2026-10-05). Both need the operator's credentials and a real
call; no agent holds either, and fixtures do not count.

## Clears when

The operator runs one subscription turn (token resolved through the secrets library) with the
live-run example and its report is recorded as evidence on story:anthropic-access.

## Also recorded here

Adversary finding F4 (review-result:adversary-w48-llm-anthropic-access-pass-1): nothing in llm keeps
a subscription-oauth binding to its operator. beyond10x/llm-gateway must not serve a subscription
route to other callers; the quoted terms forbid it.
