---
title: Price recorded usage
sidebar_position: 6
description: Quote a price book against fixture observations, and read the six separate totals.
lede: A quote prices exactly what was observed, and an unknown quantity stays unknown.
source: crates/llm-cost/examples/quote.rs, examples/prices.toml, examples/usage.json
---

# Price recorded usage

```bash
cargo run --locked -p b10x-llm-cost --example quote
```

It uses [fictional prices](https://github.com/beyond10x/llm/blob/main/examples/prices.toml) and
[fixture observations](https://github.com/beyond10x/llm/blob/main/examples/usage.json), prints an
estimate of `0.00022 USD`, and makes no provider call. These fixtures are standalone; they are not
a qualified price book for the routing example's catalog.

```rust
use llm_cost::{AccountingInput, PriceBook};

let prices = PriceBook::parse_toml(prices_toml)?;
let usage = AccountingInput::parse_json(usage_json)?;
let report = prices.quote(&usage)?;
```

## Attribution must match

A model price row names its serving-model id, immutable binding revision and expected upstream
model. An attempt retains the selected binding and the actual upstream model its producer reported.
The price context must match **both**.

A missing reported model, a different model, a repointed binding or a missing price produces a
**named unknown line** — never a silent substitution. A requested route alias is never used in
place of an absent observed model.

## Reading a report

Each line carries its unit, an optional quantity, an optional amount and an optional unknown
reason. Each total exposes `known_subtotal`, `unknown_lines` and `complete_total`.

When `final_usage = false`, calculable amounts still appear in `known_subtotal`, but the otherwise
priced lines carry `usage-incomplete` and increment `unknown_lines`. So even a fully populated
partial snapshot never produces a `complete_total`. Finality is transferred from the observation;
it must not be inferred from success, failure or populated counters. A failed attempt can have
final usage, and a terminal response can still omit counters.

## Price identity

Price rows are sorted by id before hashing validated canonical JSON. Source, revision, as-of time,
currency, all rates and binding selectors contribute to a returned SHA-256 identity. Reordering
rows or writing equivalent decimals preserves that identity; changing a price or its provenance
changes it. The as-of time records the operator's source date — it is not an automatically enforced
expiry.

## The boundary

The quote function does not perform budget admission, reservations, persistence, restart recovery
or uncertain-charge reconciliation. Those belong to the
[spending ledger](../concepts/accounting.md#the-optional-spending-ledger), which is a separate
opt-in feature with its own storage and ownership contract.
