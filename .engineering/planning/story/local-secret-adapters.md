---
format: aep.planning-md/1
id: story:local-secret-adapters
kind: story
status: draft
title: Optional local secret adapters resolve explicit references
relations:
- decomposes: epic:access
- depends_on: story:secret-resolver
scope:
- confidence: inferred
  path: crates/llm-credentials
revision: 2
---
## Context

Backend feature selection must not affect remote embeddings. Use protected file checks and mocked keychain tests; document runtime injection for remote services. Secret entry never goes through argv, TOML, tracing or diagnostic output.

## Acceptance

Keychain and explicit mounted-file adapters resolve the same SecretRef contract and refuse absent or unsafe sources without any ambient credential search.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-credentials` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
