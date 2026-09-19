# Usage and pricing verification — 2026-09-19

`llm-cost` now implements exact versioned prices, cache-partitioned token valuation, explicit
resource-millisecond pricing, attribution and separate recorded charges. The [contract](../pricing.md)
states the units, rounding, unknowns and caller-observation limits. It implements the fixtures in
`story:usage-pricing`; it does not implement durable spending admission or live provider billing.

This checkpoint was published at `15a61675339d7d2e1c2fb6aa1dc3fcd4c7e38912`.
The later [budget verification](budgets.md) extends its suite without changing this historical report.

## Counted behavior

The suite increased from **52 to 97 scenarios**, retaining every existing routing and secret
scenario. It adds 44 authored pricing cases and one generated observation check. Three consecutive
restored-source local runs each report **97 passed, 0 failed, 0 error, 0 unsupported, 0 skipped**.
The suite has 93 authored scenarios, four generated checks and zero synthesis refusals. No skip
became a failure and no fictional branch was deleted.

The retained [report](pricing-report.json) is paired with
[the exact published suite](https://github.com/beyond10x/llm/blob/15a61675339d7d2e1c2fb6aa1dc3fcd4c7e38912/contracts/suite.json). The pinned ESS 0.26.0 runner admits original
suite bytes, executes the real libraries and re-admits its report. The committed baseline holds
answered/total floors of 97 and a skipped ceiling of zero. The gate also requires complete declared
coverage and zero failures/errors/unsupported observations. It compares three consecutive counts
and regenerates the suite plus 69 schema files, refusing byte or file-set drift. No quarantine exists.

The accounting observation view exposes actual quantities, amounts, unknown reasons, record IDs,
attribution preservation, price provenance and per-basis totals. Quantities cross the ESS view
as exact decimal strings, preserving the full unsigned 64-bit runtime range; the large-count
scenario observes `18446744073709551614` without rounding. Its target invokes production
parsers and `PriceBook::quote`, and never reads expected assertions or scenario names. Fixtures
include independently calculated decimal amounts and canonical price digests. The domain's event
and observation retention belong to the test adapter, not a production event bus or durable ledger.

## Falsification

Eleven deliberate production mutations each fail a named authored scenario, with zero target
errors, unsupported observations or skips. Every source was restored byte-for-byte. The
[retained evidence](pricing-falsification.json) records exact edits, source hashes, counts and names.

| Deliberate defect | Named failure | Failed / passed |
| --- | --- | --- |
| Turn unknown quantities into zero | `unknown-cached-input-tokens` | 6 / 91 |
| Bill cached input twice | `reported-disjoint-partitions-and-reasoning` | 14 / 83 |
| Bill reasoning again as output | `reported-disjoint-partitions-and-reasoning` | 16 / 81 |
| Drop failed attempts | `failed-ambiguous-attempt-is-counted` | 1 / 96 |
| Ignore binding revision | `repointed-binding` | 1 / 96 |
| Charge subscription token valuation | `subscription-fee-is-separate` | 2 / 95 |
| Omit recorded charges | `recorded-charge-kinds-stay-separate` | 4 / 93 |
| Use seconds in a millisecond rate | `compute-millisecond-time-basis` | 5 / 92 |
| Round down | `round-up-one-nanounit` | 1 / 96 |
| Report a wrong price revision | `price-revision-is-attributed` | 26 / 71 |
| Saturate aggregate overflow | `aggregate-overflow-refused` | 1 / 96 |

## Verification scope

`task check` passes 62 unique runtime tests and two compile-fail documentation tests, default and
all-feature credentials checks, formatting, strict Clippy, ESS and AEP validation. The pricing
example also executes its local fixture. Unit tests additionally cover exact amount extremes,
currency syntax, row-order identity, version rejection and checked arithmetic. Six preexisting
immutable review records retain their missing-findings warnings.

CI runs the same checks on `ubuntu-latest` and retains original suite/report/schema artifacts as
`foundation-conformance`. Check its exact published revision and downloaded counts before claiming
remote completion. The native credential jobs remain separate macOS/Windows compilation and mock
tests. No paid provider or hosting call occurs in these gates.

This establishes pricing of explicit fixture observations, not real invoice accuracy. Actual model
identity still needs preservation through protocol integration. Producers own observation accuracy,
stable IDs, resource-interval nonoverlap and invoice reconciliation. Estimates, reference values and
recorded charges remain separate; no grand invoice total or enforced spending ceiling is claimed.
The full foundation, qualification, release and consumer migrations remain outstanding.
