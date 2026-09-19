---
format: aep.planning-md/1
id: story:neutral-inference
kind: story
status: active
title: A caller runs a neutral model turn
relations:
- decomposes: epic:inference
- depends_on: story:runtime-contracts
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-core
revision: 5
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

## Implementation progress

Implemented the async object-safe Model/StreamSink boundary, bounded text and tool turns,
monotonic cancellation, fully bound opaque provenance, independently optional usage, safe typed
errors, and versioned turn/outcome envelopes. No execution permissions or consumer dependencies
were ported. The embedding example runs without a gateway or credentials.

`task check` passed on 2026-09-19, including 13 core tests and the locked dependency-boundary
check within that count. The eight embedding fixtures exercise acceptance and failure behavior.
`cargo run --locked -p b10x-llm-core --example embedded` also passed. See
`docs/verification/core-foundation.md` for exact sources. Keep active while the parent runtime
contract's configuration and release prerequisites are completed; no provider is qualified here.
