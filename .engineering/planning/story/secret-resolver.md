---
format: aep.planning-md/1
id: story:secret-resolver
kind: story
status: draft
title: Inference accepts caller-injected secret custody
relations:
- decomposes: epic:access
- depends_on: story:neutral-inference
scope:
- confidence: inferred
  path: crates/llm-credentials
revision: 2
---
## Context

Own opaque SecretRef, redacted short-lived values and typed unavailable/expired failures. No required keychain, vault, filesystem, Connectors or login. Caller owns refresh and persistent writes; coordinate concurrent refresh requests and define retry limits before any authenticated resend.

## Acceptance

An embedding application supplies its own resolver and refresh facility and rotates a referenced credential between requests without changing configuration or exposing secret bytes.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-credentials` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
