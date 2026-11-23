---
format: aep.planning-md/3
id: story:test-binaries-one-per-crate
kind: story
status: draft
title: Each crate links one integration-test binary
summary: Consolidate 242 test files into one tests/main.rs per crate
relations:
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

Each crate of the workspace links one integration-test binary, `tests/main.rs` with one module
per former file, as the workspace rule "Tests stay fast" asks.

## Why

On 0.25.0 the workspace holds 242 integration-test files, each its own binary: `llm-messages` 45,
`llm-responses` 43, `llm-chat` 24, `llm-models` 19, `llm-bedrock` 18, `llm-core` 17, `llm-http`
16, `llm-credentials` 15, `llm-routing` 14, and fewer in the rest. Every binary links the crate
and its dependencies again, which is most of the test phase's build time and disk.
`story:gate-test-budget-and-planning-skip` bounded the test phase and removed debug information
from test builds; it left the consolidation to this story because it moves every test file.

## Acceptance

- Every crate with integration tests has exactly one test binary: `tests/main.rs` with modules
  under `tests/main/` (or one directory per crate), and no other `tests/*.rs` file.
- The number of tests `cargo nextest list --workspace` reports is unchanged, and every test name
  the invariants table of `AGENTS.md` cites is updated to its new path.
- The test phase's build of the test binaries (`cargo nextest run --workspace --no-run`, cold
  cache) is measured before and after, and both times are recorded as evidence.

## ESS first

None: test layout only.
