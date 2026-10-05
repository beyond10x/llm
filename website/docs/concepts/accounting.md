---
title: Accounting
sidebar_position: 6
description: Exact decimal money, six separate totals, and an unknown that stays unknown.
lede: Usage is priced in exact decimals over six separate bases, and a quantity nobody reported stays unknown.
source: crates/llm-cost, docs/pricing.md, docs/budgets.md
---

# Accounting

`llm-cost` values caller-observed usage and retains recorded charges. It performs no I/O and ships
no current vendor price table.

| Document | Format | Purpose |
| --- | --- | --- |
| Price book | `llm.prices/1`, TOML or JSON | Explicit source, revision, as-of time, currency and model/resource rates |
| Observations | `llm.usage/2` | Attributed attempts, resource durations and recorded charges |
| Derived report | `llm.cost/2` | Per-observation lines, known subtotals, unknowns and separate bases |

Unknown versions and fields are refused. Each input is bounded to 1 MiB and 4096 entries.

## Money is exact

Amounts are quoted, non-negative decimal strings with at most nine fractional digits, stored as
`u64` nanounits. Floats, signs, exponents, leading zeros, whitespace, excess precision and overflow
all refuse. Currency is an explicit three-letter code; there is no registry lookup and no exchange
conversion.

A rate declares an `amount` per a positive integer `per_units`. Model quantities are tokens;
compute quantities are milliseconds of one explicitly priced resource, so an hourly rate uses
`per_units = 3600000`. Multiplication uses `u128`, and division rounds each line up once to a
nanounit so that a reader can reproduce a total by adding its displayed parts.

## Unknown is never zero

`Usage.input_tokens` includes the disjoint cache-read and cache-creation subsets; output tokens
include the reasoning subset. Missing cache quantities stay **unknown**, even where another wire
treats an absent field as zero — that wire's adapter must normalize its own semantics before
producing a neutral observation.

- A known zero quantity costs zero, if the price context matches.
- An unknown quantity stays unknown even at a zero rate.
- A missing rate for a non-zero quantity is unknown.
- Contradictory known counts refuse the quote rather than saturating a negative remainder.

Every total exposes `known_subtotal`, `unknown_lines` and `complete_total`. The last is present
**only** when every contributing line is known, so a zero known subtotal with unknown lines never
claims the whole cost was zero. An empty observation list makes no statement about unobserved
activity.

## Six bases, and deliberately no grand total

| Basis | What it says |
| --- | --- |
| `metered-estimate` | Model usage multiplied by declared rates for metered billing |
| `compute-estimate` | Observed resource milliseconds multiplied by declared resource rates |
| `reference-usage` | Token valuation for subscription or self-hosted inference; not an extra charge |
| `recorded-metered` | An explicitly supplied metered charge, or an unknown charge amount |
| `recorded-compute` | An explicitly supplied compute charge, or an unknown charge amount |
| `subscription-charge` | An explicitly supplied fixed subscription charge, or an unknown amount |

There is no grand total across these bases because an estimate and a recorded charge can describe
the same work. Measured token counts produce a rate-based estimate, never a measured invoice. The
library trusts caller observations; it does not authenticate bills.

Failed, rejected and ambiguously dispatched attempts stay in the report with their original
request, target, binding, billing kind and usage. No failure flag removes an incurred or unknown
cost.

## The optional spending ledger

Enabling `llm-cost`'s `sqlite` feature adds `SqliteLedger`, a durable single-owner journal for one
trusted deployment. One immutable `BudgetPolicy` fixes the ledger id, currency, monetary limit and
maximum outstanding reservations; a different directory or policy id is a different administrative
scope. There is no period rollover, reset or FX conversion.

1. **Reserve** records an id, operation attribution, amount, assumption reference and expiry.
   Admission sums known charges and the remaining amounts held by every outstanding reservation,
   with an inclusive cap. Unknown spend or an unresolved shutdown blocks admission — including a
   zero reservation.
2. **Begin** returns a `DispatchPermit` only after the journal commit. The receipt is neither
   cloneable nor serializable, starting the same reservation twice refuses, and the permit grants
   no tool-execution, cloud-account or credential authority.
3. **Settle** records a known amount, completeness, currency and evidence reference. Previously
   known cost cannot be reduced by reconciliation, and an overrun is *retained* — admission stops
   rather than discarding it.

A started operation that expires becomes **uncertain**, not free; caller or provider evidence, not
an expired local timer, resolves that. A stop obligation records that a resource *must* be stopped;
it never proves that provider billing stopped. This library has no cloud control plane and cannot
itself turn a GPU off.
