---
format: aep.planning-md/3
id: story:codex-renewal-test-load-independent
kind: story
status: implemented
title: The Codex renewal resend test decides its claim under load
summary: adversary_codex_renewal's dropped-resolve case failed once at load average 31 on a 500 ms loopback bound
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: crates/llm-credentials/tests/adversary_codex_renewal.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T07:47:11Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-20T07:47:11Z", actor: "human:timo", revision: 5}
- {from: "active", to: "implemented", at: "2026-10-20T13:49:44Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

Seen once in wave 2026-10-20-w09 (the deltas unit's gate run): `b10x-llm-credentials`
`adversary_codex_renewal::a_dropped_resolve_does_not_let_the_next_resolve_resend_the_grant` failed
with "the first grant reached the endpoint, left: 0, right: 1" while the machine's load average was
31; it then passed 5 of 5 alone and 10 of 10 in a full rerun. The case gives a request 500 ms to
reach a loopback endpoint. The unit had no change under `crates/llm-credentials`.
`story:http-transport-load-sensitivity` covers the same kind of deadline in the HTTP transport.

## Outcome

The case decides its claim without depending on how fast a loaded machine schedules a request.

## Acceptance

- The case waits on an event (the endpoint seeing the request) rather than a fixed 500 ms, or its
  bound is justified in a comment with the measurement.
- It passes 20 of 20 under a parallel workspace build.

## Scope

Derived 2026-10-20 by `story-scoper` at 7873aa63 (tag 0.8.0). Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-credentials` tests only — cited
- **Files:** `crates/llm-credentials/tests/adversary_codex_renewal.rs` — cited
- **Symbols:** `a_dropped_resolve_does_not_let_the_next_resolve_resend_the_grant` (`:250-281`); its bound `tokio::time::timeout(Duration::from_millis(500), …)` (`:265`); the assertion that failed, "the first grant reached the endpoint" (`:267-271`) — cited
- **Also likely:** the file's `Endpoint` helper (`:109-176`) raises a `tokio::sync::Notify` after `count.fetch_add` (`:134`), raced against the resolve with `tokio::select!`; precedent `crates/llm-credentials/tests/injected.rs:87,103,123` — inferred
- **Documents:** none — inferred
- **Confidence:** high; the workspace tokio already enables `sync`, `macros` and `time` (`Cargo.toml:38`) — cited
- **Would collide with:** any unit editing `crates/llm-credentials/tests/adversary_codex_renewal.rs`; disjoint from `story:http-transport-load-sensitivity` — cited
- **Safety fact:** `resolve_now` records `RefreshUncertain` before it awaits the grant (`crates/llm-credentials/src/codex/renewal.rs:586-592`), so dropping on the endpoint's read keeps the second-resolve assertions (`:273-280`) meaningful — unproven
