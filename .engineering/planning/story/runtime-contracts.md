---
format: aep.planning-md/1
id: story:runtime-contracts
kind: story
status: draft
title: Publish the neutral contract and compatibility policy
relations:
- decomposes: epic:contracts
scope:
- confidence: inferred
  path: .github/workflows
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: Taskfile.yml
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec
revision: 2
---
## Context

Refine the ESS declaration model into typed protocol/auth/capability vocabularies before runtime code. Define async streaming/cancellation and ownership, opaque provenance, optional usage, protocol-neutral tools without authority, serialization stability and supported text/tool scope. Preserve current source provenance. Pin AEP/ESS/toolchain and gate/release workflows; no provider calls in ordinary CI.

## Acceptance

A versioned contract declares the supported turn, configuration, error and compatibility surfaces and all dependency-boundary checks pass.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `spec` — planned implementation surface.
- inferred: `docs/design.md` — planned implementation surface.
- inferred: `Cargo.toml` — planned implementation surface.
- inferred: `Cargo.lock` — planned implementation surface.
- inferred: `.github/workflows` — planned implementation surface.
- inferred: `Taskfile.yml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
