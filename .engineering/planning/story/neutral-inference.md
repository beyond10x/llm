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
- confidence: cited
  path: checks/conformance
- confidence: cited
  path: contracts
- confidence: cited
  path: crates/llm-core
- confidence: cited
  path: docs
- confidence: cited
  path: spec
revision: 9
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

## Required pricing integration

The implemented pricing story accepts caller-observed upstream model attribution separately from
the selected binding. Current TurnOutcome carries usage counts but no actual model attribution.
Before qualifying provider-to-price integration, the neutral/protocol contract must preserve the
model observed on the wire and make it available to accounting. Never substitute a requested route
alias or selected internal model ID as evidence of the model actually served. Coordinate any
versioned envelope change through runtime-contracts before protocol implementation; the pending
Chat task is not silently widened by this note. See story:usage-pricing and docs/pricing.md.

## Bound result observations

A TurnObservation pins the selected immutable binding and carries independently optional upstream
model, response ID and normalized Usage. It never substitutes the request alias, binding's internal
model ID or configured upstream name for absent provider evidence. final_usage states that reported
counters are terminal; it does not make missing counters known or certify an invoice. Protocol
adapters normalize a consistent snapshot and preserve it on cancellation, transport, sink and
protocol failures. Provider-final usage can accompany a later output-validation failure.

TurnOutcome carries this observation and validates its binding, finality and usage. Error carries
an optional boxed observation, preserving safe fixed diagnostics and independent dispatch evidence.
An observation on a not-sent error is contradictory and refuses validation. Local pre-dispatch
errors carry none. Observations grant no execution authority and contain no raw upstream errors.

Change the unreleased output envelope to llm.outcome/3 and explicitly refuse older/future formats;
llm.turn/2 is unchanged. Cost AttemptUsage adds required final_usage; llm.usage/2 and llm.cost/2
replace the unreleased v1 usage/cost envelopes. A partial consistent snapshot retains its known
quantities and calculable valuation as a lower bound, but every such line remains incomplete.
Known subtotals never become complete totals merely because all snapshot fields are populated.
Failed attempts with genuinely final reported counters remain priceable. Unknown model, rate and
quantities keep their existing explicit reasons. Pricing still never authenticates invoices.

The validated llm.inference domain gives these observations a typed home; the accounting domain
adds explicit finality. Add real parser/validation and price-total ESS observations and authored
fixtures, keep all prior scenarios, and falsify model substitution, binding loss, missing partial
usage and incorrect completion. Update the embedding example and old-reader compatibility checks.
No lifecycle moves beside the pending Chat driver and no widening of that governed task. No live
model or hosting calls in the ordinary gate. Messages adapter implementation follows this contract.

## Observation implementation evidence

Implemented bound TurnObservation in successful outcomes and optional boxed failure evidence.
Actual upstream model/response IDs stay absent when unreported; partial/final usage is independent
of success. Validation rejects foreign binding coordinates, contradictory usage and unsent evidence.
Unreleased envelope versions are turn/2 (unchanged), outcome/3, usage/2 and cost/2. Partial prices
retain known lower bounds and never complete a total, including fully populated/all-zero snapshots.

Verification: task check exits 0; 183/183 ESS scenarios pass three consecutive runs, 177 authored and
six generated, zero failed/error/unsupported/skipped/refused; all 89 schemas regenerate exactly.
Ten deliberate production mutations fail named scenarios, restored byte-for-byte. See
 docs/verification/observations.md, observations-report.json and observations-falsification.json.
Source identity: llm-foundation-libraries sources-sha256:372762ca11728e8cdccf7fe8fc82d04f207ecb99485d9a116a868aa4295c4f2e
Spec digest: 1e4410acbb59104a27f7fbdde1f33de64703f1b86743bfd6ff773fcc1d5ca823
Suite: sha256:0d7fe71de43ee8f7dd9ed3b41f939da99725886e045b12f10db105b999596f6e

No lifecycle moves beside the pending Chat driver. No provider/gateway/hosting integration or
remote qualification is claimed by this checkpoint; Messages implementation follows.
