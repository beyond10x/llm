---
format: aep.planning-md/1
id: story:modal-hosting
kind: story
status: draft
title: Modal supplies the hosting contract without simulated capabilities
relations:
- decomposes: epic:hosting
- depends_on: story:hosting-contract
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts/modal
- confidence: inferred
  path: crates/llm-modal
revision: 2
---
## Context

Verify the current official deployment/control contract before choosing a Rust integration boundary. Do not invent a general REST deployment API or treat a pre-existing endpoint as provisioning proof. Capture any unsupported control-plane action as a blocker; use existing-endpoint routing independently. No new Python runtime without an explicit language decision.

## Acceptance

A documented Modal control-plane binding passes lifecycle fixtures and separately records a live deployment/readiness/cleanup result for its supported lifecycle.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-modal` — planned implementation surface.
- inferred: `contracts/modal` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
