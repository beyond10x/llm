---
format: aep.planning-md/1
id: story:provider-accounts
kind: story
status: active
title: Provider accounts are independent of wire selection
relations:
- decomposes: epic:access
- depends_on: story:secret-resolver
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-providers
revision: 5
---
## Context

Define provider facts, account/billing identity, endpoint, auth kind and protocol as distinct values. Include caller-managed API/subscription sources and anonymous explicitly selected local endpoints; no built-in-name restriction. Validate configuration without resolving secrets.

## Acceptance

A caller binds arbitrary provider/account IDs to endpoints while independently selecting a supported protocol and model. An explicitly anonymous binding succeeds without a credential reference or resolver call; every authenticated binding requires a reference and refuses a missing reference during configuration validation. No automatic downgrade to anonymous access is permitted.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-providers` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
