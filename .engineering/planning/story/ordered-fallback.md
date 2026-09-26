---
format: aep.planning-md/1
id: story:ordered-fallback
kind: story
status: draft
title: Fallback tries only explicitly compatible alternatives
relations:
- decomposes: epic:routing
- depends_on: story:catalog-routing
- depends_on: story:spending-limits
- depends_on: story:responses-projection
- depends_on: story:messages-projection
- depends_on: story:chat-projection
scope:
- confidence: inferred
  path: crates/llm-routing
revision: 2
---
## Context

Define eligible typed failure classes and bounded attempt/deadline rules. No secret-source fallback, silent model/billing downgrade or dropped requested setting. Every attempted route contributes certainty/usage/cost evidence; callers can disable fallback.

## Acceptance

Scripted failures produce only the declared ordered attempts before visible output and demonstrate refusal after partial output, incompatible opaque state, ambiguous dispatch or exhausted limits.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-routing` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
