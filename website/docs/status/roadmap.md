---
title: Roadmap
description: The next implementation step, and the order the remaining work has to happen in.
---

# Roadmap

The ordering is not arbitrary. Each item below is blocked by the one above it for a stated reason.

## Done: the protocol projections

Responses, Messages and Chat Completions are implemented on top of the shared bounded transport,
each with its own conformance suite and falsification record. The constraints the
[contract](../concepts/neutral-boundary.md) fixed in advance are the ones they are held to:

- Independent authentication, billing and protocol choices survive translation. An anonymous vLLM
  binding is expressible without a special case.
- Unsupported fields and provider-specific opaque continuation state are preserved or refused —
  never dropped to make a translation look successful.
- Each adapter normalizes its wire's usage counts before producing neutral values, and may report a
  partial quantity only when it is a valid lower bound.

## Next: provider access qualification

`openai-access` and `anthropic-access` need API **and** caller-managed subscription presentation,
each with its own compatibility evidence. Subscription access must never silently fall back to
billable API credentials. Qualification is live evidence; it cannot be produced by the offline gate.

## Then: ordered runtime fallback

`ordered-fallback` needs the projections first, because eligibility is defined in terms of what a
real attempt exposed. Only defined failures before output becomes visible are eligible, an
ambiguously accepted request is never replayed as though it were free, and every attempt is
recorded with its unknown spend preserved.

This is also where pricing and budget admission join the attempt loop: the effectful consumers
still need to take the ledger's permits and honour its stop obligations.

## Then: the hosting adapters

`hosting-contract` is done: `llm-provision` models owned-resource lifecycle, leases,
reconciliation and cleanup, with identity qualified by the provider's `incarnation` so two
controllers can never own one billed resource, and a stop obligation nothing but evidence
discharges. It opens no socket and allocates nothing; an in-process `FakeProvider` demonstrates
the lifecycle.

`runpod-hosting` and `modal-hosting` follow behind that seam and are still five-line stubs.
Cloud-specific capabilities need documented control-plane evidence; an unsupported lifecycle
action is reported, never simulated. Nothing in this repository has yet allocated or stopped a
real GPU.

## Then: gateway translation and the operator CLI

`gateway-auth` is done: `llm-gateway` admits one authenticated owner, decodes nothing past the
HTTP head before acceptance, serves a read-only route inventory with no field for an endpoint URL
or a secret reference, and starts, drains and stops deliberately.

`gateway-translation` is not, and the crate says so itself — its module documentation lists
"protocol translation, proxying a model call, resolving a secret, reaching a network" among the
things it refuses. Exposing the three protocol ingress surfaces over the published neutral subset
is the remaining work, and it needs the projections it now has.

`operator-cli` validates, inspects and runs one configuration — inspection resolving no secret
and provisioning no resource. `llm-cli` is still a five-line stub.

## Finally: a qualified release

`foundation-qualified` needs an exact release, its required checks and artifacts, and every
implementation and qualification evidence item above. Harness and Metaharness adopt a released or
explicitly qualified exact revision afterwards; `llmgw` keeps operating until a reversible cutover
retires it.

## Explicitly out of scope for this milestone

Product-level cheapest-model optimization, and the `connectors-secret-resolver` adapter, which
waits for [Connectors](https://beyond10x.github.io/docs/connectors/) to support arbitrary secret
custody.
