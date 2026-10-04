---
format: aep.planning-md/3
id: story:gateway-translation
kind: story
status: draft
title: The gateway translates only the supported protocol subset
relations:
- decomposes: epic:gateway
- depends_on: story:gateway-auth
- depends_on: story:ordered-fallback
- depends_on: story:runpod-hosting
- depends_on: story:modal-hosting
- depends_on: story:unattributed-opaque-state
scope:
- confidence: inferred
  path: contracts/gateway
- confidence: inferred
  path: crates/llm-gateway
revision: 3
---
## Context

Use shared projections/routing/costs and optional hosting. Publish exact supported ingress endpoints and semantics, not a claim of complete vendor API equivalence. No downstream tool execution. Apply deadlines and cleanup on client disconnect; test fallback visibility boundary.

## Acceptance

A three-protocol client/upstream test matrix preserves text, tools, streaming, cancellation and usage or explicitly refuses unsupported fields and opaque state.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-gateway` — planned implementation surface.
- inferred: `contracts/gateway` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.

## Opaque state carried from ingress

Carried from wave 3, `review-result:adversary-opaque-pass-2` finding 2. Ingress now carries opaque
state it cannot attribute as `Item::UnattributedOpaque`, and binding it is the caller's explicit
decision (`TurnRequest::bind_unattributed`). A gateway that binds every carried entry to the binding
that read the request reinstates the laundering `story:unattributed-opaque-state` closed: a payload
minted under an earlier binding revision becomes sendable to the current one. The conformance
adapter does exactly that for its round-trip scenario (`checks/conformance/src/responses.rs:186-192`).
This story must decide which binding a gateway may bind carried state to, and refuse the rest.
