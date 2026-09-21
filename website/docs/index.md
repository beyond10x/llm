---
slug: /
title: Composable model inference
description: A neutral model-inference boundary with injected credentials, bounded transport, explained routing and accounting that never turns an unknown into a zero.
---

# LLM

LLM is a Rust workspace that separates model inference from the agent loop that calls it. It owns
the neutral turn, the credential injection point, the transport, the provider bindings, the routing
catalog and the accounting. It does not own tool execution, approval envelopes, sandboxing or
delegation; those stay with the calling harness.

:::caution Implemented, but not qualified and not released
Eleven of the fourteen crates are implemented, including all three protocol projections. What is
**not** here: the operator command line, both cloud hosting adapters, protocol translation in the
gateway, and any qualified provider access — **no live provider credential has been used anywhere
in this repository**, so nothing has been proven against a real endpoint. There is no release.
[Where this stands](status/where-this-stands.md) lists every story and its evidence.
:::

## What the boundary is for

The repository exists to remove a specific class of quiet defect from model clients:

| Defect | What this boundary does instead |
| --- | --- |
| A missing usage counter is billed as zero | Every usage field is independently optional; unknown is never zero |
| The configured model name is reported as the served one | The observed upstream model is an independent optional observation; a route alias is never substituted |
| A transport failure after dispatch is retried as free | Dispatch evidence is `not-sent`, `rejected`, `unknown` or `accepted`; `unknown` is not proof of a free retry |
| Opaque provider state is replayed against a different target | Opaque items carry protocol, provider, account, endpoint, model and binding revision; all six are checked |
| A subscription credential silently falls back to a billable key | Billing kind is independent of protocol and authentication and never changes on rejection |
| An error message leaks the upstream body | Diagnostics name a fixed code and carry no headers, request bodies or arbitrary upstream text |

## Where to go next

- [Getting started](getting-started.md) builds the workspace and runs the three examples that work
  today without a provider account.
- [The neutral boundary](concepts/overview.md) explains the five concepts the libraries keep apart.
- [Where this stands](status/where-this-stands.md) is the story-by-story implementation record.
- [Limitations](status/limitations.md) states the trust boundary and what local evidence does not
  establish.
