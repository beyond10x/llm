---
format: aep.planning-md/1
id: story:usage-pricing
kind: story
status: draft
title: Usage and versioned prices produce attributable estimates
relations:
- decomposes: epic:routing
- depends_on: story:neutral-inference
scope:
- confidence: inferred
  path: crates/llm-cost
- confidence: inferred
  path: spec/domains/catalog.yaml
revision: 2
---
## Context

Specify decimal/integer money, currency, time basis, token cache disjointness, price provenance/revision, estimate versus measured charges and aggregation across attempts. Model optional usage and cost certainty in ESS. No hardcoded current vendor price claims and no unknown-to-zero conversion.

## Acceptance

Fixtures price reported token/cache/compute usage against versioned sources while preserving unknown quantities, subscription charges and failed-attempt costs separately.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-cost` — planned implementation surface.
- inferred: `spec/domains/catalog.yaml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
