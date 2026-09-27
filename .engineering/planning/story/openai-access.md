---
format: aep.planning-md/2
id: story:openai-access
kind: story
status: draft
title: OpenAI API and caller-managed subscription routes are qualified
relations:
- decomposes: epic:access
- depends_on: story:provider-accounts
- depends_on: story:responses-projection
scope:
- confidence: inferred
  path: contracts/openai
- confidence: inferred
  path: crates/llm-providers
revision: 2
---
## Context

Caller provides credentials and renewal. Never run login or rewrite a vendor-owned auth document. Record fresh vendor contract sources, emulated fixtures and separately authorized live evidence. Do not imply ChatGPT subscription is general API credit; no fallback to API credentials.

## Acceptance

Separate API and subscription integration reports identify exact endpoint/auth contracts, successful supported model turns, rotation behavior and unsupported cases without crossing billing accounts.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-providers` — planned implementation surface.
- inferred: `contracts/openai` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
