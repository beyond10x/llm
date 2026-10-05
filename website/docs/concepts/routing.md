---
title: Routing
sidebar_position: 5
description: A strict TOML catalog, ordered opt-in selection, same-target retry, ordered fallback, and an explanation that resolves no secret.
lede: A route names its targets in order; a run retries a target only while nothing is visible, and falls back only to targets the route named.
source: crates/llm-routing (catalog, explain, fallback.rs), examples/catalog.toml
---

# Routing

`llm-routing` parses a strict `llm.catalog/1` TOML document and explains capability-aware selection
without performing any I/O. It also runs a turn across a route's targets, retrying and falling back
through model ports the caller supplies.

## What a catalog declares

Providers, accounts, endpoints, models, serving models, routes and ordered targets are separate
tables with operator-defined ids. There is no built-in-name-only resolver: nothing resolves because
a string happened to look like a vendor's model name.

```toml
format = "llm.catalog/1"

[[accounts]]
id = "remote"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "metered"
secret_reference_id = "lab-llm-token"

[[serving_models]]
id = "remote-large"
endpoint_id = "remote-models"
model_id = "large"
protocol = "responses"
[serving_models.capabilities]
tools = true
context_window = 32768
max_output_tokens = 8192
```

## Selection is ordered and opt-in

`fallback_enabled` defaults to **false**; omission is false. When it is off, only the first target
of a route is eligible and every other candidate is reported with the rejection reason
`fallback-disabled`. When it is on, selection may take the first compatible target among the
explicitly named alternatives — never a target the route did not name. A route with fallback on
cannot mix a `subscription-oauth` target with a target under other billing; the catalog refuses it,
so a turn never moves from a subscription to metered or self-hosted billing.

Admission requires a caller-supplied **input-token upper bound** that must be valid for every
candidate. An unknown input count refuses admission rather than guessing.

## Explanation is safe by construction

The explanation exposes route id, alias, configuration digest, the selected target, and for each
candidate its provenance, authentication kind, billing kind, declared capabilities and rejection
reasons. It contains no prompt, no opaque payload and no secret, because explaining resolves
nothing.

See [Explain a route](../guides/explain-a-route.md) for the real output of the shipped example.

## Same-target retry

A failure that may be retried (`Error::may_retry`, see [the neutral turn](neutral-boundary.md#whether-a-failure-may-be-retried))
and that offered the caller's sink nothing is tried again **on the same target** before the run
moves on. `RetryPolicy` sets how, and `FallbackPolicy::default()` turns it on:

| Field | Default | Meaning |
| --- | --- | --- |
| `max_attempts` | 4 | Attempts per target, the first included; 1 turns retry off; at most 16 |
| `backoff_base` | 500 ms | Doubled per attempt made: waits of 1, 2 and 4 seconds |
| `max_doublings` | 4 | The wait stops growing after this many doublings |
| `max_server_delay` | 30 s | A server's `Retry-After` is honoured up to this, never below the local wait |

Before each wait the run states a warning with the code `turn-retried` on the caller's sink. The
wait races cancellation, no wait starts that would end at or after the caller's deadline, and the
caller's `admit` runs again before the retry. A retried attempt may have been billed; it stays in
the record with its own dispatch evidence.

## Ordered fallback

`Catalog::run_turn` runs a request against a route. It tries the route's targets in declared
position order and skips every target that `explain` rejects. With `fallback_enabled = false`,
`explain` rejects every alternative as `fallback-disabled`, so only the first target can run.
With it on, the run moves to the next compatible target only when the failed attempt offered no
event to the caller's sink **and** either

1. its failure may be retried and the target's retries are spent, or
2. its dispatch evidence is `not-sent` or `rejected`, belongs to the attempt's binding, and its
   error is `transport`, `rate-limited` or `unavailable`.

Everything else ends the run. That includes `unauthorized` (there is no fallback to another
credential source or account), `invalid-request`, `refused`, `cancelled`, `deadline`, and an
`unknown` dispatch whose class may not be retried, which is never replayed.

```rust
pub async fn run_turn(
    &self,
    request: &TurnRequest,
    input_tokens: Option<u64>,
    policy: FallbackPolicy,   // max_attempts, an optional deadline, and retry
    ports: Ports<'_>,         // models, admit, sink, cancel, pause
) -> Result<FallbackRun, Error>
```

The caller supplies everything effectful through `Ports`:

| Port | What it is for |
| --- | --- |
| `models` | Maps a serving-model id to the single-attempt `Model` that serves it, such as a `ResponsesClient` |
| `admit` | Called before every attempt, the first and every retry included. This is where a spending limit refuses; routing itself does not depend on `llm-cost` |
| `sink` | Receives the stream of whichever attempt is running, and the `turn-retried` warnings |
| `cancel` | Cancels the run |
| `pause` | Waits between attempts on one target; routing owns no timer, so a Tokio caller passes `&\|wait\| Box::pin(tokio::time::sleep(wait))` |

Before the first attempt, every target the run could try must have a model whose provenance is
exactly that target's binding; otherwise the run is refused. Before every attempt the run checks,
in order, the attempt bound, cancellation, the deadline and admission. `FallbackPolicy::disabled()`
allows one attempt in total, with neither fallback nor retry; to keep retry without fallback, set
`max_attempts: 1` on `FallbackPolicy::default()`.

The returned `FallbackRun` records every started attempt: its target, binding, authentication and
billing kind, how many events it made visible, and its own observation, failures included. Usage an
attempt did not report stays unknown. `halt` says why the run stopped:

| `halt` | Meaning |
| --- | --- |
| `succeeded` | An attempt completed |
| `exhausted` | No compatible target remained |
| `ineligible-failure` | The failure class does not permit fallback |
| `visible-output` | Output had already reached the caller |
| `ambiguous-dispatch` | The request may have been accepted, so it is not replayed |
| `limit-refused` | `admit` refused the next target |
| `deadline`, `cancelled` | The caller's deadline passed, or the caller cancelled |
| `attempt-bound` | `max_attempts` was reached |

Fallback never moves to a target the route did not name, never degrades a request's capabilities to
fit, and never falls back after a partial stream.
