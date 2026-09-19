---
format: aep.planning-md/1
id: story:runtime-contracts
kind: story
status: active
title: Publish the neutral contract and compatibility policy
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: .github/workflows
- confidence: cited
  path: AGENTS.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: cited
  path: README.md
- confidence: inferred
  path: Taskfile.yml
- confidence: cited
  path: docs/contract-v1.md
- confidence: inferred
  path: docs/design.md
- confidence: cited
  path: docs/implementation-status.md
- confidence: cited
  path: docs/verification/core-foundation.md
- confidence: inferred
  path: spec
revision: 6
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

## Implementation progress

Implemented `docs/contract-v1.md`, typed protocol/auth/billing/capability vocabularies in
`spec/domains/catalog.yaml`, and dependency-boundary checks. The turn and outcome envelopes
reject unknown versions and fields. `task check` passed on 2026-09-19; exact fixture identities
and limits are in `docs/verification/core-foundation.md`.

Remaining: versioned provider/routing configuration and source release/common Gates setup.
Keep this story active; a compiling workspace or local library tests do not qualify a release.
Current complete-milestone tracking is `docs/implementation-status.md`.

Additional cited scope: `README.md`, `AGENTS.md`, `docs/contract-v1.md`,
`docs/implementation-status.md`, and `docs/verification/core-foundation.md` describe the
implemented contract and its verification without claiming planned capabilities.
