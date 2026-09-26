---
format: aep.planning-md/1
id: story:hosting-contract
kind: story
status: draft
title: Hosting has explicit owned-resource lifecycle semantics
relations:
- decomposes: epic:hosting
- depends_on: story:provider-accounts
- depends_on: story:spending-limits
scope:
- confidence: inferred
  path: crates/llm-provision
- confidence: inferred
  path: spec/domains/catalog.yaml
revision: 2
---
## Context

Resolve UNMAPPED hosting transitions before implementation. Separate DeploymentSpec from observed ProvisionedDeployment. Document mutation ambiguity, idempotency, cancellation, ownership fencing, max resources/time and cost-policy integration. Listing/validation must never allocate compute.

## Acceptance

A fake hosting provider demonstrates provisioning, readiness, concurrent acquisition, active leases, restart reconciliation and cleanup against a validated ownership state machine.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-provision` — planned implementation surface.
- inferred: `spec/domains/catalog.yaml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.
