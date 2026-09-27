---
format: aep.planning-md/2
id: story:usage-pricing
kind: story
status: implemented
title: Usage and versioned prices produce attributable estimates
relations:
- decomposes: epic:routing
- depends_on: story:neutral-inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: checks/conformance
- confidence: cited
  path: contracts
- confidence: cited
  path: crates/llm-cost
- confidence: cited
  path: docs/pricing.md
- confidence: cited
  path: examples/prices.toml
- confidence: cited
  path: examples/usage.json
- confidence: cited
  path: spec
- confidence: cited
  path: spec/domains/inference.yaml
revision: 14
---
## Context

Specify decimal/integer money, currency, time basis, token cache disjointness, price provenance/revision, estimate versus measured charges and aggregation across attempts. Model optional usage and cost certainty in ESS. No hardcoded current vendor price claims and no unknown-to-zero conversion.

## Acceptance

Fixtures price reported token/cache/compute usage against versioned sources while preserving unknown quantities, subscription charges and failed-attempt costs separately.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Implemented `llm-cost`: exact decimal/nanounit arithmetic; versioned JSON/TOML price input with
source, revision, timestamp and deterministic identity; actual-model/binding checks; disjoint
cache pricing; resource-millisecond estimates; attributed unknowns; and separate per-basis
subtotals for estimates/reference usage/recorded charges. Duplicate observations, mixed currency,
invalid usage/time intervals and arithmetic overflow refuse. Failed/ambiguous attempts are retained.

`task check` passed 62 unique runtime tests and two compile-fail documentation tests, feature
checks, fmt/strict Clippy, ESS and AEP validation. The local quote example prints 0.00022 USD from
fictional rates and observations. The combined ESS suite has 97 passes, zero failed/error/
unsupported/skipped in three consecutive restored-source runs; 93 authored and four generated,
zero synthesis refusals, 69 byte-checked schemas. All preceding 52 scenarios remain selected.

Eleven restored production mutations each fail a named authored pricing scenario, with zero
target errors, unsupported observations or skips. Retained details are docs/verification/pricing.md,
pricing-report.json and pricing-falsification.json, paired with contracts/suite.json. Both the
quantities/amounts and canonical price digests have independent fixture expectations.

This proves the story's pure fixture pricing behavior. Caller observations are not authenticated
invoices; producers own actual model attribution, stable IDs, interval nonoverlap and reconciliation.
Protocol/core integration must preserve the observed model before an end-to-end pricing claim.
No price is hardcoded as current, and no paid call or resource deployment occurs. Budget admission,
durable reservations/restart and full-foundation qualification remain outstanding. No lifecycle
move is made beside the pending Chat drive. Exact remote CI is checked after publication.

## Scope

- cited: `crates/llm-cost` — exact money, validated documents, quote/aggregation and tests/example.
- cited: `spec`, `contracts`, `checks/conformance` — normalized concepts, real quote observations,
  authored scenarios, deterministic projections and report gate; shared with runtime-contracts.
- cited: `Cargo.lock`, `examples/prices.toml`, `examples/usage.json` — dependencies and executable fixture input.
- cited: `docs/pricing.md`, `docs/verification/pricing.md` — usage, contracts, evidence and limits.

## Implementation contract

The implementation is a pure Rust library, independent of a gateway, provider clients and budget
storage. It adapts the source/provenance and per-record integer arithmetic from Harness price.rs;
it deliberately preserves the neutral core's optional counts rather than defaulting them to zero.
ESS accounting declarations were validated before this design was recorded. Existing catalog Price
and UsageRecord remain referenced declaration owners; accounting adds their rate/usage detail.

Use exact nonnegative decimal strings with nine fractional digits maximum, represented by u64
nanounits; reject floats, exponent notation, negatives, excess precision and overflow. A currency
is an explicit three-uppercase-letter code, not a currency-registry lookup. Rates declare an
amount and positive integer denominator. Compute quantity is milliseconds of one explicitly
priced resource, not inferred GPU time. Checked u128 multiplication and division round each line
up to one nanounit; record totals sum rounded lines, and aggregate totals sum records. No FX.

Versioned price input (llm.prices/1, JSON or TOML) contains source, revision, as-of Unix milliseconds,
currency and explicit model/compute rows. Validate duplicate IDs, bounds and denominator; canonical
identity sorts rows and hashes validated data. Model rows bind serving-model ID, binding revision
and expected upstream model. Quote input (llm.usage/1) preserves caller-observed actual upstream
model independently of selected binding; absent or mismatched actual model remains unknown.
Protocol projections must supply this observation for later end-to-end pricing qualification.

The quote validates neutral Usage partitions. Input total includes cache read and creation;
reasoning is already included in output. Missing cache quantities cannot become zero, and known
invalid partitions refuse. Each line keeps quantity, amount or a named unknown reason. Known zero
quantity can be priced at zero without its rate, but an unknown quantity remains unknown even at a
zero rate. Missing/mismatched price context is unknown, not permission to apply another row.

Every observation has an explicit unique ID and attribution. Preserve failed and uncertain
attempts without filtering. Metered token estimates, compute estimates, reference token valuations,
recorded metered/compute charges and subscription charges have separate subtotal buckets. Token
valuation on subscription/self-hosted billing is reference value, not another monetary charge.
Recorded charges carry their own source/account/subject/time window and optional known amount;
they are never multiplied by inference count. Reject mixed currencies and duplicate observation
IDs. Expose known subtotal plus unknown-line count and only expose a complete bucket total when
every line is known. Do not add estimate and recorded buckets into a misleading grand total.

Use real production quote facts in ESS views; test all named failure/unknown boundaries and mutate
production rules to prove scenarios can fail. Retain all 52 existing scenarios. No vendor price is
baked in, no paid call runs, and budget reservations/restart/uncertain-spend ownership remain with
story:spending-limits. No artifact lifecycle moves beside the pending Chat drive.

## Partial usage integration

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
