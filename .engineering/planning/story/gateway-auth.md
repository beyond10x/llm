---
format: aep.planning-md/1
id: story:gateway-auth
kind: story
status: draft
title: The gateway admits one authenticated owner
relations:
- decomposes: epic:gateway
- depends_on: story:catalog-routing
scope:
- confidence: inferred
  path: crates/llm-gateway
revision: 2
---
## Context

Compose injected credential sources and verifier for local or remote single-owner deployment; never assume a desktop keychain on servers. Authenticated context is supplied before business decoding. Define health/readiness and graceful shutdown. Multi-tenant accounts/quotas are outside this milestone.

## Acceptance

Unauthenticated requests are refused and an authenticated owner can inspect routes without revealing upstream credentials or triggering provisioning.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-gateway` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
