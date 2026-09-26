# Attributed usage and prices

The pricing APIs in `llm-cost` value caller-observed usage and retain recorded charges.
These APIs perform no I/O and supply no current vendor price table. The optional
[budget ledger](budgets.md) owns separate admission and storage behavior. Pricing inputs and outputs are:

| Document | Format | Purpose |
| --- | --- | --- |
| Price book | `llm.prices/1`, TOML or JSON | Explicit source, revision, as-of Unix milliseconds, currency and model/resource rates |
| Observations | `llm.usage/2`, JSON or Rust values | Attributed attempts, resource durations and recorded charges |
| Derived report | `llm.cost/2`, JSON or Rust values | Per-observation lines, known subtotals, unknowns and separate accounting bases |

Unknown versions and fields are refused. Each input is bounded to 1 MiB and 4096 entries.
Source descriptions accept nonblank Unicode text up to 2048 bytes, without control characters.
`PriceBook::from_document` and `PriceBook::quote` also validate programmatically constructed data.
The derived report is serializable; durable budget storage and report ingestion belong to the
later spending-ledger contract. Observations and exact price input can be retained and repriced.

Run `cargo run --locked -p b10x-llm-cost --example quote`. It uses
[fictional prices](../examples/prices.toml) and [fixture observations](../examples/usage.json),
prints an estimate of `0.00022 USD`, and makes no provider call. These price bindings are standalone
fixtures, not a qualified price book for the separate catalog example.

```rust
use llm_cost::{AccountingInput, PriceBook};

let prices = PriceBook::parse_toml(prices_toml)?;
let usage = AccountingInput::parse_json(usage_json)?;
let report = prices.quote(&usage)?;
```

## Exact money and explicit units

Amounts are quoted, nonnegative decimal strings, with at most nine fractional digits. They are
stored as `u64` nanounits; the maximum is `18446744073.709551615` currency units. Floats, signs,
exponents, leading zeros, whitespace, excess precision and overflow refuse. Serialization emits a
canonical decimal string. Currency is an explicit three-uppercase-letter code; there is no
currency-registry lookup or exchange-rate conversion.

A rate declares `amount` per a positive integer `per_units`. Model quantities are tokens.
Compute quantities are milliseconds of one explicitly priced resource, such as a whole pod.
A rate per hour therefore uses `per_units = 3600000`. Resource class, hosting provider and account
must match the selected compute row; the library never infers GPU count or a billing clock from
an inference request. Supply a separate observation for each measured resource interval. The
producer owns interval measurement and nonoverlap; a duration is not a live billing attestation.

Multiplication uses `u128`. Division rounds each line upward once to a nanounit; record and bucket
totals sum those rounded lines. Readers can reproduce a total by adding its displayed parts.
Different per-record grouping can change this rounding, so records retain their original identity.
Arithmetic beyond the supported range refuses instead of saturating or wrapping.

## Model attribution, cache partitions and unknowns

A model price names its serving-model ID, immutable binding revision and expected upstream model.
An attempt retains the selected binding and the actual upstream model reported by its producer.
The price context must match both. A missing reported model, a different model, repointed binding
or missing price produces named unknown lines. A requested route alias is never substituted for
an absent observed model.

`Usage.input_tokens` includes cache reads and cache creation. The uncached quantity is the total
minus both cache classes, after validating that known subsets fit. Reasoning tokens are already
part of output and are not charged twice. Missing cache quantities stay unknown, even if another
wire treats an absent field as zero; that wire's adapter must establish and normalize its own
semantics before supplying a neutral observation. This version has one cache-creation price per
model row; producers must not collapse differently priced cache classes into that quantity.

Each line contains its unit, optional quantity, optional amount and an optional unknown reason.
A known zero quantity costs zero even if its rate is absent, provided the price context matches.
An unknown quantity remains unknown even at a zero rate. Missing rates for nonzero quantities are
unknown. Contradictory known counts, invalid rates or invalid documents refuse the quote.

Each total exposes `known_subtotal`, `unknown_lines`, and `complete_total`. The last is present only
when every contributing line is known. A zero known subtotal with unknown lines never says the
whole cost is zero. An empty observation list has no totals and makes no statement about unobserved
activity.

`TurnOutcome.observation` and `Error.observation` carry the selected immutable binding, optional
actual upstream model/response IDs, optional usage and explicit `final_usage`. Consumers transfer
that finality to `AttemptUsage`; they must not infer it from success/failure or populated counters.
A failed attempt can have final usage, and a terminal response can still omit some counters.
Protocol adapters own consistent snapshot normalization and may only report partial quantities
that are valid lower bounds. Repeated cumulative snapshots replace prior snapshots; they are not
separate billable attempts.

For `final_usage = false`, calculable amounts remain in `known_subtotal`, with `usage-incomplete`
on otherwise priced lines. Such lines also increment `unknown_lines`, so even fully populated or
all-zero partial snapshots never produce a `complete_total`. The uncached input line is the one
exception to "calculable": it is a difference, and lower bounds of the total and of each cache
class do not bound their difference, because a later snapshot can move input into a cache class.
A partial snapshot therefore prices that line `quantity-unknown` with no quantity or amount; only
a final cache split supplies it. Every other partial line is a directly reported, monotone
quantity, so `known_subtotal` stays a lower bound of the attempt's final `complete_total`. Existing missing model, quantity and
rate reasons remain when a line cannot be priced. `llm.usage/2` requires explicit finality and
refuses v1; `llm.cost/2` reflects amounts and incomplete reasons coexisting on a line. The price
book remains v1. These are unreleased contract changes, not implicit compatibility conversions.
The library accepts caller observations; its fixtures do not qualify a provider or supply missing
provider metadata.

## Charges and estimates stay separate

Every observation has a unique ID; duplicate IDs across all observation kinds refuse. Failed,
rejected and ambiguously dispatched attempts remain in the report with their original request,
target, binding, billing kind and usage. No failure flag removes an incurred or unknown cost.

Totals have six bases:

| Basis | What it says |
| --- | --- |
| `metered-estimate` | Model usage multiplied by declared rates for metered billing |
| `compute-estimate` | Observed resource milliseconds multiplied by declared resource rates |
| `reference-usage` | Token valuation for subscription or self-hosted inference; not an additional charge |
| `recorded-metered` | Explicitly supplied metered charge or unknown charge amount |
| `recorded-compute` | Explicitly supplied compute charge or unknown charge amount |
| `subscription-charge` | Explicitly supplied fixed subscription charge or unknown amount |

Recorded charges retain source, provider, account, subject and a nonempty half-open period in Unix
milliseconds. They are added once per observation, not once per inference. They must use the book's
currency. A producer must use stable observation IDs and reconcile repeated/overlapping invoice
items; changing an ID does not establish that another charge occurred.

There is deliberately no grand total across these bases: an estimate and a recorded charge can
describe the same work. Measured token counts produce a rate-based estimate, not a measured invoice.
The library trusts caller observations; it does not authenticate bills or verify the real-world
accuracy of a model's claims. Budget admission, reservations, persistence, restart and uncertain
charge reconciliation remain the spending-limit story, not a property of this quote function.

## Price identity and specification

Price rows are sorted by ID before hashing validated canonical JSON. Source, revision, as-of time,
currency, all rates and binding selectors contribute to the returned SHA-256 identity. Reordering
rows or writing equivalent decimals preserves identity; changing a price or provenance changes it.
The as-of time records the operator's source date; it is not an automatically enforced expiry.

ESS catalog `Price` and `UsageRecord` remain declaration owners. Accounting adds normalized detail:
book metadata is shared by model/compute rows, model-rate detail references its catalog price,
and attempt detail references its usage record. These entity schemas describe the normalized
concepts, not a claim that a flat TOML row contains a second copy of its book's source/revision.
The accounting observation command invokes the real parsers and quote function; its view exposes
actual quantities, amounts, unknown reasons, attribution preservation and totals. See
[verification](verification/pricing.md) for counted and mutation evidence.
