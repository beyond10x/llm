---
format: aep.planning-md/3
id: story:runpod-hosting
kind: story
status: implemented
title: Runpod provides recoverable vLLM deployments
relations:
- decomposes: epic:hosting
- depends_on: story:hosting-contract
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: crates/llm-runpod/Cargo.toml
- confidence: cited
  path: crates/llm-runpod/src
- confidence: cited
  path: crates/llm-runpod/src/lib.rs
- confidence: cited
  path: crates/llm-runpod/tests
- confidence: cited
  path: docs/hosting.md
- confidence: cited
  path: docs/verification/runpod-falsification.json
- confidence: cited
  path: docs/verification/runpod.md
revision: 10
transitions:
- {from: "draft", to: "proposed", at: "2026-09-26T15:24:23Z", actor: "human:timo", revision: 6, imported: true}
- {from: "proposed", to: "active", at: "2026-09-26T15:24:24Z", actor: "human:timo", revision: 7, imported: true}
- {from: "active", to: "implemented", at: "2026-09-26T16:29:10Z", actor: "human:timo", revision: 10, decided_on: {"recorded":{"test_result":1,"review_outcome":15}}, imported: true}
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
