---
format: aep.planning-md/1
id: story:spending-limits
kind: story
status: draft
title: Spending admission accounts for concurrency and uncertainty
relations:
- decomposes: epic:routing
- depends_on: story:usage-pricing
- depends_on: story:catalog-routing
scope:
- confidence: inferred
  path: crates/llm-cost
- confidence: inferred
  path: spec/domains/catalog.yaml
revision: 2
---
## Context

First resolve UNMAPPED budget ownership and scope in an ESS design. Define reservations, settlement, provider-accepted ambiguity, persistence/restart behavior, fixed subscription versus marginal charges and compute shutdown obligations. Label estimate-based policy honestly; do not claim a hard provider-invoice ceiling.

## Acceptance

A deterministic ledger simulation demonstrates that concurrent requests and provisioned resources cannot bypass the declared spending-admission policy, including unknown usage and restart.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-cost` — planned implementation surface.
- inferred: `spec/domains/catalog.yaml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
