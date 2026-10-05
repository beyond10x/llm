---
format: aep.planning-md/3
id: story:lints-rust-1-99
kind: story
status: draft
title: The llm workspace lints clean on rustc 1.99
relations:
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

The workspace lints clean on the current stable rustc, not only on the pinned 1.98.0.

## Why

Wave 2026-10-05-w22: `cargo clippy --workspace --all-targets --all-features -- -D warnings` on rustc 1.99.0 reports locations in files that predate the wave: `llm-gateway/tests/gateway.rs:758`, `llm-provision/tests/adversary_ownership.rs:592`, eight in `llm-provision/tests/hosting.rs`, `llm-runpod/src/pool.rs:50` (deprecated `fetch_update`), and `llm-routing/tests/fallback.rs:426`, `:543`, `fallback_adversary.rs:203`, `fallback_adversary_2.rs:250`. CI pins 1.98.0 (`.github/workflows/gate.yml:15`), so CI is green.

## Acceptance

- The workspace clippy exits 0 on rustc 1.99.0 and on the pinned toolchain; the pin moves only with a decision.
