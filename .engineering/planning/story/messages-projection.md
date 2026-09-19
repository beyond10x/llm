---
format: aep.planning-md/1
id: story:messages-projection
kind: story
status: draft
title: Messages projects the supported neutral subset
relations:
- decomposes: epic:inference
- depends_on: story:http-streaming
scope:
- confidence: inferred
  path: contracts/messages
- confidence: inferred
  path: crates/llm-messages
revision: 2
---
## Context

Outgoing and ingress codecs share the contract. Authentication presentation is supplied by provider binding; API keys and caller-owned OAuth are not inferred from wire choice. Reject unsupported fields rather than copying provider-specific state across models.

## Acceptance

Pinned Messages request and stream fixtures preserve tools, thinking state, cache usage and failures across the declared subset.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-messages` — planned implementation surface.
- inferred: `contracts/messages` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
