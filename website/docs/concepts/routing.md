---
title: Routing
description: A strict TOML catalog, ordered opt-in selection and fallback, capability admission, and an explanation that resolves no secret.
---

# Routing

`llm-routing` parses a strict `llm.catalog/1` TOML document and explains capability-aware selection
without performing any I/O. It also runs a turn across a route's targets with ordered fallback, through model ports the caller supplies.

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
explicitly named alternatives — never a target the route did not name.

Admission requires a caller-supplied **input-token upper bound** that must be valid for every
candidate. An unknown input count refuses admission rather than guessing.

## Explanation is safe by construction

The explanation exposes route id, alias, configuration digest, the selected target, and for each
candidate its provenance, authentication kind, billing kind, declared capabilities and rejection
reasons. It contains no prompt, no opaque payload and no secret, because explaining resolves
nothing.

See [Explain a route](../guides/explain-a-route.md) for the real output of the shipped example.

## Ordered fallback

`Catalog::run_turn` runs a request against a route. It tries the route's targets in declared
position order and skips every target that `explain` rejects. With `fallback_enabled = false`,
`explain` rejects every alternative as `fallback-disabled`, so only the first target can run.
With it on, the run moves to the next compatible target only when
**all three** of these hold for the failed attempt:

1. It offered no event to the caller's sink. Nothing was visible yet.
2. Its dispatch evidence is `not-sent` or `rejected`, and that evidence belongs to the attempt's
   binding.
3. Its error is `transport`, `rate-limited` or `unavailable`.

Everything else ends the run. That includes `unauthorized` (there is no fallback to another
credential source or account), `invalid-request`, `refused`, `cancelled`, `deadline`, any
`accepted` dispatch, and any `unknown` dispatch, which is never replayed.

```rust
pub async fn run_turn(
    &self,
    request: &TurnRequest,
    input_tokens: Option<u64>,
    policy: FallbackPolicy,   // max_attempts and an optional deadline
    ports: Ports<'_>,         // models, admit, sink, cancel
) -> Result<FallbackRun, Error>
```

The caller supplies everything effectful through `Ports`:

| Port | What it is for |
| --- | --- |
| `models` | Maps a serving-model id to the single-attempt `Model` that serves it, such as a `ChatClient` |
| `admit` | Called before every attempt, the first included. This is where a spending limit refuses; routing itself does not depend on `llm-cost` |
| `sink` | Receives the stream of whichever attempt is running |
| `cancel` | Cancels the run |

Before the first attempt, every target the run could try must have a model whose provenance is
exactly that target's binding; otherwise the run is refused. Before every attempt the run checks,
in order, the attempt bound, cancellation, the deadline and admission. `FallbackPolicy::disabled()` allows one attempt, whatever the route says.

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
