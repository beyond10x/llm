---
format: aep.planning-md/3
id: story:quota-429-is-final
kind: story
status: draft
title: An HTTP 429 a provider documents as an exhausted quota is final
summary: The transport retries every 429 and reads no body, so an insufficient_quota 429 is retried
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
revision: 1
---
## Context

Recorded by the implementor of `story:retry-classes-harness-retried` in wave 2026-10-20-w09: the
HTTP transport marks every 429 and every 500-599 retriable (`crates/llm-http/src/transport.rs`,
around `:336-340`) and reads no error body, so an OpenAI 429 whose body says `insufficient_quota`
(an exhausted quota, which the provider documents as not resolved by retrying) is retried.
`docs/design.md` records this limit since that wave.

## Outcome

An HTTP 429 that a provider documents as an exhausted quota is final.

## Acceptance

- The transport or the protocol client reads a bounded error body for 429 and classifies
  `insufficient_quota` as final, without copying provider prose into a diagnostic.
- A test per protocol pins a quota 429 as final and a rate-limit 429 as retriable.
