---
format: aep.planning-md/1
id: story:unattributed-opaque-state
kind: story
status: draft
title: A gateway carries opaque state it cannot attribute, and cannot send it
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-chat
- confidence: inferred
  path: crates/llm-core
- confidence: inferred
  path: crates/llm-messages
- confidence: inferred
  path: crates/llm-responses
- confidence: inferred
  path: docs/contract-v1.md
revision: 2
---
## Context

A gateway reading a request body cannot tell where the opaque continuation state in it came from.
The wire carries no provenance, and the neutral model has exactly one representation for such
state: bound to its exact provider, account, endpoint, model and binding revision.

That leaves an ingress surface two choices, and both are wrong.

**Stamp it with the binding doing the reading.** This is what the Responses projection did first,
and it launders. A payload minted under one binding revision, replayed by a client after the
endpoint was repointed, comes back out addressed to the new revision. State the outgoing path
refuses becomes state the outgoing path sends, in one pass. The adversary demonstrated exactly that
round trip. It contradicts the invariant the repository states plainly: a mismatch is refused,
never silently reused.

**Refuse it.** Sound, and what the projection now does. It costs reasoning continuity across a tool
round trip through a gateway: where the provider does not store state server-side, its reasoning
item is exactly such an entry, and a client that cannot replay it makes the model re-derive its
plan on every call.

The projection took the second, because a conservative refusal is honest and a stamp is not. The
cost is stated in its own documentation rather than hidden. This story is the third option.

## Acceptance

An ingress surface can carry opaque continuation state it could not attribute, that state is not
sendable, and binding it to a target is an explicit decision a caller makes rather than one an
adapter makes silently. A gateway round trip preserves the payload; an attempt to forward it
without that decision is refused by name.

## Evidence

`story:responses-projection` correction round 1, and the adversary finding that produced it:
ingress stamped every unmodelled entry with the reading binding, so state a repointed binding
refuses on egress was reinstated as native. Recorded in
`review-result:adversary-responses-pass-1`, finding 3. The projection's own analysis and the shape
it proposes are retained at `docs/plan/wave-1-unattributed-opaque-request.md`.

## Verification

The envelope version moves, because an older reader must not silently accept a new variant of a
published type. A scenario shows the payload surviving a gateway round trip byte for byte; another
shows a forward attempt refused by name; another shows the explicit binding decision succeeding.
A deliberate defect that reinstates the stamping behaviour fails a named scenario. No paid provider
call runs in the ordinary gate.

## Scope

- inferred: `crates/llm-core` — the added state and the envelope version.
- inferred: `docs/contract-v1.md` — the sentence in turn ownership that says such state is carried,
  is not sendable, and is bound only by an explicit caller decision.
- inferred: `crates/llm-responses`, `crates/llm-chat`, `crates/llm-messages` — each ingress path
  carries rather than refuses.
- inferred: `contracts` — the round trip, the refusal and the explicit binding.

Two things this must not do, both named by the projection that raised it: do not widen provenance
with a sentinel revision, and do not make the comparison partial. Each makes a mismatch
representable as a match, which is the failure the whole rule exists to prevent.

This lands after wave 1. It changes a crate three projections depend on, and all three were being
written at once.
