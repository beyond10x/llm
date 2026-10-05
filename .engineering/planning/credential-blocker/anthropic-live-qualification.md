---
format: aep.planning-md/3
id: credential-blocker:anthropic-live-qualification
kind: credential-blocker
status: open
title: The live API and subscription runs need the operator's credentials
relations:
- blocks: story:anthropic-access
revision: 1
---
## What is missing

The story's acceptance asks for a live qualification report per route: one supported model turn
over the Anthropic API with an API key, and one over the operator's own subscription token, each
naming the endpoint and auth contract it used. Both need the operator's credentials and a real
call; no agent holds either, and fixtures do not count.

## Clears when

The operator runs one turn per route (API key; subscription token resolved through the secrets
library) and the two reports are recorded as evidence on story:anthropic-access.

## Also recorded here

Adversary finding F4 (review-result:adversary-w48-llm-anthropic-access-pass-1): nothing in llm keeps
a subscription-oauth binding to its operator. beyond10x/llm-gateway must not serve a subscription
route to other callers; the quoted terms forbid it.
