---
format: aep.planning-md/1
id: story:chat-projection
kind: story
status: draft
title: Chat Completions projects the supported neutral subset
relations:
- decomposes: epic:inference
- depends_on: story:http-streaming
scope:
- confidence: inferred
  path: contracts/chat
- confidence: inferred
  path: crates/llm-chat
revision: 2
---
## Context

Outgoing and ingress codecs cover the published subset; missing usage remains absent and unsupported features fail by name. Provide a vLLM-compatible fixture, not a fabricated live proof.

## Acceptance

Pinned Chat Completions fixtures preserve streamed text, tool arguments, usage and finish reasons through the neutral interface.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-chat` — planned implementation surface.
- inferred: `contracts/chat` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
