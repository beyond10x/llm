---
format: aep.planning-md/1
id: story:catalog-routing
kind: story
status: draft
title: TOML resolves and explains capability-compatible routes
relations:
- decomposes: epic:routing
- depends_on: story:provider-accounts
scope:
- confidence: inferred
  path: crates/llm-routing
- confidence: inferred
  path: spec/domains/catalog.yaml
revision: 2
---
## Context

Refine Route/RouteTarget and model-capability ESS definitions. Ordered targets carry explicit protocol, endpoint/model and reference-only auth; context and setting support must agree. The explain result exposes safe facts, configuration identity and why alternatives were rejected.

## Acceptance

A TOML catalog containing arbitrary providers and models resolves a deterministic route or names the incompatible capability without contacting secrets, inference or hosting.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-routing` — planned implementation surface.
- inferred: `spec/domains/catalog.yaml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
