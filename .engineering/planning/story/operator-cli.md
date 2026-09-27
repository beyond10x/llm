---
format: aep.planning-md/2
id: story:operator-cli
kind: story
status: draft
title: Operators validate, inspect and run one LLM configuration
relations:
- decomposes: epic:gateway
- depends_on: story:gateway-translation
- depends_on: story:local-secret-adapters
scope:
- confidence: inferred
  path: crates/llm-cli
- confidence: inferred
  path: docs/examples
revision: 2
---
## Context

Use clap and structured safe diagnostics. Provide local-keychain and remote-mounted-source examples, inspect capabilities and pricing certainty, and clearly separate provisioning mutations from read-only discovery. CLI is not a second routing implementation.

## Acceptance

CLI fixtures validate/explain routes without side effects and start the authenticated gateway from the same TOML and injected adapter composition.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-cli` — planned implementation surface.
- inferred: `docs/examples` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
