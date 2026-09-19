---
format: aep.planning-md/1
id: story:neutral-inference
kind: story
status: draft
title: A caller runs a neutral model turn
relations:
- decomposes: epic:inference
- depends_on: story:runtime-contracts
scope:
- confidence: inferred
  path: crates/llm-core
revision: 2
---
## Context

Implement the runtime-contracts decision. Preserve absent usage and opaque continuation state; no permissions, loop state, tool execution or consumer dependencies. Provide an embedding example without a gateway.

## Acceptance

A fake model implementation completes streaming text and tool round trips with cancellation and typed failures through the public neutral interface.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-core` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
