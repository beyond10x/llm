---
format: aep.planning-md/1
id: story:responses-projection
kind: story
status: draft
title: Responses projects the supported neutral subset
relations:
- decomposes: epic:inference
- depends_on: story:http-streaming
scope:
- confidence: inferred
  path: contracts/responses
- confidence: inferred
  path: crates/llm-responses
revision: 2
---
## Context

Support both outgoing calls and gateway ingress projection using one protocol contract. Retain unknown events/opaque items or refuse explicitly. Include existing Harness/vLLM reasoning-event evidence instead of claiming all Responses behavior.

## Acceptance

Pinned request and streaming fixtures round-trip the supported Responses text/tool subset and reject incompatible continuation state without losing usage or terminal truth.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-responses` — planned implementation surface.
- inferred: `contracts/responses` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
