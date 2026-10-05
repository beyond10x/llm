---
format: aep.planning-md/3
id: story:anthropic-access
kind: story
status: active
title: Anthropic API and caller-managed subscription routes are qualified
relations:
- decomposes: epic:access
- depends_on: story:provider-accounts
- depends_on: story:messages-projection
- depends_on: story:secrets-resolver
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts/anthropic
- confidence: inferred
  path: contracts/messages
- confidence: inferred
  path: crates/llm-messages
- confidence: inferred
  path: crates/llm-providers
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: spec
- confidence: inferred
  path: website/docs
revision: 10
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T13:03:52Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"review_outcome":1}}}
- {from: "proposed", to: "active", at: "2026-10-05T13:03:52Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"review_outcome":1}}}
---
## Context

API access uses Messages. Caller-managed subscription is requested but must be independently qualified; retain current credential-use restrictions and any required access evidence, never collect vendor login credentials or silently substitute API billing. Full milestone qualification remains blocked if the requested route cannot be established.

## Acceptance

Separate API and subscription reports each demonstrate a successful supported model turn, identify the exact endpoint/auth contract, verify protocol behavior and credential rotation, and name unsupported cases. Completion requires both reports; an unavailable subscription path leaves this story and foundation qualification incomplete with a named blocker. No API-billing substitution satisfies subscription qualification.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-providers` — planned implementation surface.
- inferred: `contracts/anthropic` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
