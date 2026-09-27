---
format: aep.planning-md/2
id: story:catalog-routing
kind: story
status: implemented
title: TOML resolves and explains capability-compatible routes
relations:
- decomposes: epic:routing
- depends_on: story:provider-accounts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: contracts/routing
- confidence: inferred
  path: crates/llm-routing
- confidence: cited
  path: examples/catalog.toml
- confidence: inferred
  path: spec/domains/catalog.yaml
- confidence: cited
  path: spec/domains/routing.yaml
revision: 8
---
## Context

Refine Route/RouteTarget and model-capability ESS definitions. Ordered targets carry explicit protocol, endpoint/model and reference-only auth; context and setting support must agree. The explain result exposes safe facts, configuration identity and why alternatives were rejected.

## Acceptance

A TOML catalog containing arbitrary providers and models resolves a deterministic route or names the incompatible capability without contacting secrets, inference or hosting.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-routing` — planned implementation surface.
- inferred: `spec/domains/catalog.yaml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.

## Routing contract

Implement a strict llm.catalog/1 TOML document with declared providers/accounts/endpoints/models,
serving models, routes and ordered route targets. Existing entity relations remain in
spec/domains/catalog.yaml; RouteRejection, TargetExplanation and RouteExplanation now have typed
homes there. Canonical configuration identity hashes validated declarations, not secrets.

A TurnRequest.model names a route alias for catalog resolution. A selected request rewrites only
that alias to the declared model ID; all content/settings and opaque state remain unchanged.
Explain checks each named target for tool/sampling support, output/context limits and exact opaque
provenance, listing reasons by capability. Input-token admission requires a caller-supplied
conservative upper bound valid for all candidates; unknown input cannot prove admission and is
explicitly refused. This is not a tokenizer or final invoice guarantee.

Targets must have unique IDs, unique serving bindings within a route and contiguous positions
starting at zero. Fallback defaults off; when off, only position zero may be selected. When on,
explain may select the first locally compatible target. Runtime failures/retries, deadlines,
spending admission and accounting belong to ordered-fallback/spending-limits. No resolver, HTTP
client or provisioner is accepted by the catalog inspection API.

Source evidence: Platform fa9de23a9b5142cd6ed54d21488d8c9dddd3b197,
runtime/inference/crates/inference-route/src/{catalog,route,vocabulary}.rs. Preserve strict
vocabularies, validated catalogs and deterministic capability refusals. Replace embedded vendor
lists and harness-owned authentication with independent operator-defined bindings.

## Implementation evidence

Implemented strict llm.catalog/1 TOML validation, canonical declaration digest, safe explanations,
explicit ordered capability selection and conservative input-token admission. Same-ID endpoint or
upstream model repointing changes binding provenance and refuses old opaque continuation state.
No resolver, HTTP client or provisioner participates in parse/explain/resolve.

Verification on 2026-09-19: full task check passes (50 Rust runtime tests, 2 compile-fail doc tests;
28 ESS scenarios in three consecutive runs, no failures/errors/unsupported/skips/refusals).
The executable ESS record is executable-system-specification:routing-observations. See
 docs/verification/routing-conformance.md and its paired report/suite. Eight temporary production
mutations fail named scenarios and were restored. This is local library evidence, not runtime
fallback, a live provider qualification, release or consumer adoption. Lifecycle remains active.
