---
format: aep.planning-md/1
id: story:secret-resolver
kind: story
status: implemented
title: Inference accepts caller-injected secret custody
relations:
- decomposes: epic:access
- depends_on: story:neutral-inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-credentials
revision: 6
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

## Implementation progress

Implemented caller-injected SecretResolver, opaque SecretRef, zeroized/redacted arbitrary bytes,
nonserializable secret material and generation identities, and bounded per-reference coordination.
Resolution observes rotation between requests. Concurrent rejection refreshes one generation
once. A cancelled refresh remains uncertain and cannot be blindly repeated until caller-owned
state changes. No login, ambient lookup, persistent writes or fallback source was added.

`task check` passed on 2026-09-19: five injected-resolver fixtures and two compile-fail tests.
See `docs/verification/core-foundation.md`. Authenticated resend limits will be exercised by the
provider clients; the resolver never sends a request. Keep active under the unfinished versioned
runtime-contract prerequisites. Local keychain/file adapters belong to their separate story.
