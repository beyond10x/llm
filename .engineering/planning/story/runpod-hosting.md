---
format: aep.planning-md/1
id: story:runpod-hosting
kind: story
status: draft
title: Runpod provides recoverable vLLM deployments
relations:
- decomposes: epic:hosting
- depends_on: story:hosting-contract
scope:
- confidence: inferred
  path: contracts/runpod
- confidence: inferred
  path: crates/llm-runpod
revision: 2
---
## Context

Port existing llmgw behavior with attribution. Ordered GPU choices, mounted caches, startup deadlines and active-stream leases remain explicit. Prevent overlap with the legacy llmgw namespace/controller. Live paid verification is separately recorded and never part of the default gate.

## Acceptance

Emulated Runpod tests prove single-flight startup, readiness/crash recovery, ownership-safe adoption and reachable idle/orphan cleanup using declared vLLM settings.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-runpod` — planned implementation surface.
- inferred: `contracts/runpod` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
