# Ordered fallback verification — 2026-09-26

`Catalog::run_turn` (`crates/llm-routing/src/fallback.rs`) attempts a route's declared targets in
position order, skipping every target `Catalog::explain` rejects. It falls back only after a
failure that

- offered no event to the caller's sink,
- carries `Dispatch::NotSent` or `Dispatch::Rejected` with evidence valid for that binding, and
- is `transport`, `rate-limited` or `unavailable`.

Every other failure ends the run: `unauthorized` (no secret-source or account fallback),
`invalid-request`, `refused`, `protocol`, `too-large`, `unsupported`, `cancelled`, `deadline`,
anything with `Dispatch::Accepted`, and any `Dispatch::Unknown` (never replayed). Since 2026-10-05 a
failure whose retry class `Error::may_retry` allows is the exception: it is retried on the same
target (`FallbackPolicy::retry`, Harness policy by default) and then falls back whatever its
dispatch, as `docs/contract-v1.md` and `spec/domains/routing.yaml` state. Before every
attempt, the first included, the run checks, in order: the caller's attempt bound
(`FallbackPolicy::max_attempts`; `FallbackPolicy::disabled()` is one target and one attempt on it,
with no same-target retry), cancellation, the caller's
deadline, and the caller's admission decision (`Ports::admit`, where limits are enforced; this crate
does not depend on `b10x-llm-cost`). Each started attempt records its target, binding, auth and
billing kind, how many events it offered, and its own observation — also when the attempt's
outcome is refused or its dispatch claim contradicts its evidence, as long as that observation is
bound to the attempt's binding. Foreign evidence is never recorded. Usage it did not report stays
unknown. The run returns an explanation naming why each skipped target was rejected.

Only targets the run could attempt — the first `max_attempts` compatible ones — are bound and need
a model. For those, a missing model or one whose provenance is not the target's binding refuses
the run before any attempt. With fallback disabled by the route or by the caller
(`FallbackPolicy::disabled()`), alternatives need no model.

## Evidence

| Lane | Command | Fixtures |
|---|---|---|
| Rust | `cargo test -p b10x-llm-routing --locked --test fallback --test fallback_adversary` | scripted `llm_core::Model` fakes, `VecSink`; no projection, no network |
| ESS | `llm.routing.Fallback` → `llm.routing.LastFallback` (`spec/domains/routing.yaml`) | `contracts/routing/scenarios/fallback-*.yaml` (10 authored) |

The ESS scenarios run through the `llm.routing.Fallback` adapter in
`checks/conformance/src/fallback.rs`. Cancellation has no scenario, because the adapter cannot
cancel; Rust cases kill its mutation. Each mutation, with the scenario and Rust cases it failed, is
recorded in [routing-falsification.json](routing-falsification.json) under the entries whose
`source` is `crates/llm-routing/src/fallback.rs`.
