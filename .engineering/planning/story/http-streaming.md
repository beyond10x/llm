---
format: aep.planning-md/3
id: story:http-streaming
kind: story
status: implemented
title: Bounded streaming transport handles termination and cancellation
relations:
- decomposes: epic:inference
- depends_on: story:neutral-inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-http
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-09-19T07:01:14Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-09-19T07:01:14Z", actor: "human:timo", revision: 4, imported: true}
- {from: "active", to: "implemented", at: "2026-09-19T19:34:25Z", actor: "human:timo", revision: 7, decided_on: {"recorded":{"test_result":3}}, imported: true}
---
## Context

Transport has no vendor fields, credential acquisition or model selection. Carry caller-supplied request bytes/headers safely, reject credential-bearing redirects and preserve partial-stream failure. Explicit request/idle/deadline bounds and attempt certainty.

## Acceptance

Deterministic HTTP/SSE fixtures demonstrate bounded decoding, cancellation, terminal truth and retry-delay handling without duplicate exposed output.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-http` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.

## Implementation progress

Implemented bounded SSE decoding and asynchronous single-attempt HTTP transport. It refuses
redirects, never retries, tracks dispatch certainty, preserves valid output before malformed
frames, and bounds response headers, idle time and total duration. Cancellation or failure drops
the HTTP response even when the stream object remains retained. Retry-After is a hint only.

`task check` passed on 2026-09-19: four framing fixtures and seven real loopback-socket fixtures,
including EOF without terminal truth, silent-stream cancellation, continuous keepalive deadlines,
credential-bearing redirect refusal and no duplicate request. See
`docs/verification/core-foundation.md`. Keep active under the unfinished versioned runtime-contract
prerequisites. Protocol-specific terminal interpretation remains with the projection stories.

## CI correction

CI run https://github.com/beyond10x/llm/actions/runs/35429528719 at b66b4256dd4e5ea37a7bed0b6dfdf73c277bf1a3
found a race between reqwest and transport deadlines, reporting Transport instead of Deadline.
Removed the redundant reqwest timeout and retained one absolute deadline. Review also corrected
EOF framing errors to retain Accepted dispatch evidence, asserted by the truncated-stream fixture.
Local task check passed; the exact deadline regression passed twelve consecutive executions.
See docs/verification/core-foundation.md. This supersedes any inference that the initial local
pass established CI success; the failed run is retained as evidence of the defect.
