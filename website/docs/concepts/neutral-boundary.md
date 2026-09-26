---
title: The neutral turn
description: One asynchronous port, bounded validation before any network I/O, and opaque state bound to its exact target.
---

# The neutral turn

`llm-core` defines what every caller and every adapter agrees on: a request, a stream of events, an
outcome, a failure. It performs no I/O and depends on no consumer.

The implemented contract is *unreleased revision 2*. It supports stateless text and tool turns.
Image and audio input, vendor-side tools, provider thread management and unrecognized request
settings are outside this version and adapters must refuse them.

## Streamed events

A turn streams `StreamEvent`s into the caller's sink: text deltas, reasoning deltas, the start of a
tool call, and fragments of that call's arguments. A tool call is announced once, with the
identifier and name the provider sent, before any of its argument fragments. The finished
`TurnOutcome` then carries the complete items, the stop reason and the observation.

## One attempt, one port

`Model::turn` is an object-safe asynchronous port taking one request, a caller-owned asynchronous
sink and a cancellation token. Its future borrows those values.

- A client performs **one** attempt. Routing owns retries, alternate accounts and fallback.
- Sink emission is awaited, so backpressure is bounded. Sink failure terminates the attempt.
- Cancellation is monotonic, cloneable, wakes pending work and wins over simultaneous completion.
- Dropping the future cancels local work. It **cannot** prove that an upstream did not accept or
  bill the request.

## Items carry no authority

An `Item` is user or assistant text, a tool call, its successful or failed result, or an opaque
provider item. A tool definition contains only a name, a description and a JSON Schema. It confers
no execution permission: LLM never executes a tool.

## Opaque state is bound to six coordinates

Opaque items carry protocol, provider, account, endpoint, model and binding revision. The contract
requires every adapter to check **all six** against the selected binding before sending —
including same-protocol cross-model and cross-account requests. There is no implicit exception; a
narrower documented exception would need a versioned contract change.

:::note Enforced by three adapters, against fixtures
The Responses, Messages and Chat Completions adapters implement this check and each carries
scenarios that assert it. Those scenarios run against fixtures and in-process fakes, so the
guarantee is verified against what the contract says a wire looks like, not against a live
provider. `llm-core` validates and refuses foreign binding coordinates on the values an adapter
hands it; it cannot recover a coordinate an adapter discarded.
:::

### State that arrives without an origin

A request that arrives at an ingress surface, such as a Messages or Responses body a client sends,
names no binding. Reasoning state inside it therefore has no provable origin. Ingress decoders hold
it as `Item::UnattributedOpaque`: the payload as a JSON value and the protocol it was read from,
and nothing else.

That item cannot be sent. Every egress path refuses it with the fixed message
`Item::UNATTRIBUTED_REFUSAL` until the caller binds it with `TurnRequest::bind_unattributed`, which
also refuses a target of a different protocol. No adapter makes that decision for the caller, and
ingress never stamps the binding that read the state onto it.

The binding revision hashes the complete validated single-binding declaration, including the
endpoint URL, upstream model, auth reference and capabilities. Repointing an existing id therefore
invalidates old opaque state. Secret bytes and credential generations are excluded, so rotating the
same reference does not invalidate a continuation.

## Validation happens before the network

Absent sampling fields remain absent. Unsupported settings refuse before any network I/O.
Identifiers and individual fields are bounded, and so is the whole serialized request. The request
model must match the selected target, and tool choices must name published tools.

Core validates structure and declared capabilities. It is not a tokenizer, and it cannot check
whether an operator's capability declaration is true. Context-window admission is the routing and
token-accounting concern; request byte limits do not prove it.

## Evidence, not inference

Every successful outcome carries a `TurnObservation` pinned to its selected immutable binding. The
upstream model and response id are **independently optional** provider observations; the configured
model and the caller's alias cannot fill absent evidence.

`final_usage` says the reported counters are terminal — not that every count is known, and not that
an invoice was authenticated. Failures may retain the last valid snapshot, which may be partial or
final. Validation refuses foreign binding coordinates, contradictory usage, and upstream evidence
attached to a `not-sent` failure. The core validates these values but cannot recover a fact an
adapter discarded.

## Failures say what they know

Errors distinguish invalid input, transport, protocol, authentication, rate limits, refusal,
bounds, unsupported semantics, cancellation and deadlines. Dispatch evidence is independent of the
error kind:

| Dispatch evidence | Meaning |
| --- | --- |
| `not-sent` | Nothing reached the upstream |
| `rejected` | The upstream refused before accepting work |
| `unknown` | The upstream may have accepted and may bill |
| `accepted` | The upstream accepted the request |

A transport failure after dispatch is `unknown`, not proof of a free retry. A server delay hint is a
hint, not permission to retry. Error diagnostics never include authorization headers, request
bodies or arbitrary upstream error text.
