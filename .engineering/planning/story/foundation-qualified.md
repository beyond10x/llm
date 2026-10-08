---
format: aep.planning-md/3
id: story:foundation-qualified
kind: story
status: draft
title: An exact LLM release is qualified for consumer adoption
relations:
- decomposes: epic:gateway
- depends_on: story:runtime-contracts
- depends_on: story:openai-access
- depends_on: story:anthropic-access
scope:
- confidence: inferred
  path: .github/workflows
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: docs/release
revision: 2
---
## Context

Require API and subscription access, three protocols/translation, injected secrets, fallback/limits and both hosting providers. Separate emulator results, live credentials and paid cloud evidence. Include fresh clean-checkout gate, documented public contracts, cancellation/redaction failures and release checks/artifacts. A pushed tag, placeholder crate or requested-but-blocked provider is not qualification. Consumers remain blocked until this evidence exists.

## Acceptance

A release report ties the complete first-milestone matrix and required artifacts to one published version and records no unresolved blocker on required foundation capabilities.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `.github/workflows` — planned implementation surface.
- inferred: `Cargo.toml` — planned implementation surface.
- inferred: `Cargo.lock` — planned implementation surface.
- inferred: `CHANGELOG.md` — planned implementation surface.
- inferred: `docs/release` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
