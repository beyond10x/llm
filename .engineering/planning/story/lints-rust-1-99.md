---
format: aep.planning-md/3
id: story:lints-rust-1-99
kind: story
status: implemented
title: The llm workspace lints clean on rustc 1.99
relations:
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-chat/tests
- confidence: inferred
  path: crates/llm-gateway/tests
- confidence: inferred
  path: crates/llm-http/tests/framing.rs
- confidence: inferred
  path: crates/llm-provision/tests
- confidence: inferred
  path: crates/llm-responses/tests
- confidence: inferred
  path: crates/llm-routing/tests
- confidence: inferred
  path: crates/llm-runpod/src/pool.rs
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T08:05:31Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T08:05:31Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T08:23:39Z", actor: "human:timo", revision: 11, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Outcome

The workspace lints clean on the current stable rustc, not only on the pinned 1.98.0.

## Why

Wave 2026-10-05-w22: `cargo clippy --workspace --all-targets --all-features -- -D warnings` on rustc 1.99.0 reports locations in files that predate the wave: `llm-gateway/tests/gateway.rs:758`, `llm-provision/tests/adversary_ownership.rs:592`, eight in `llm-provision/tests/hosting.rs`, `llm-runpod/src/pool.rs:50` (deprecated `fetch_update`), and `llm-routing/tests/fallback.rs:426`, `:543`, `fallback_adversary.rs:203`, `fallback_adversary_2.rs:250`. CI pins 1.98.0 (`.github/workflows/gate.yml:15`), so CI is green.

## Acceptance

- The workspace clippy exits 0 on rustc 1.99.0 and on the pinned toolchain; the pin moves only with a decision.
