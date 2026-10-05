---
format: aep.planning-md/3
id: story:harness-builds-on-llm
kind: story
status: implemented
title: Harness builds and passes its tests on llm in place of its own wire crates
relations:
- decomposes: epic:serving-split
- depends_on: story:harness-parity
- depends_on: story:serving-extraction
- depends_on: story:parity-retry-classes
- depends_on: story:parity-http-timeouts-cancel
- depends_on: story:parity-credential-sources
- depends_on: story:parity-codex-renewal
- depends_on: story:parity-responses-wire
- depends_on: story:parity-responses-live-stream
- depends_on: story:parity-messages-wire
- depends_on: story:parity-blocking-adapters
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T10:20:28Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T10:20:28Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T10:39:29Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Outcome

Harness compiles and passes its own test suite with `harness-messages`, `harness-responses`,
`harness-http` and `harness-credential` replaced by released llm client crates, on a branch of
`beyond10x/harness`. This proves the swap is possible; it is not a cutover.

## Acceptance

- A branch on `beyond10x/harness` in which no crate depends on the four Harness wire crates; their
  users (`harness-cli`, `harness-app-server`) depend on llm crates by release tag.
- `cargo test --workspace --locked` on that branch exits 0, and its test list (`-- --list`) is
  recorded beside the result.
- Every adapter written in Harness to bridge a type difference is listed, with its line count.
- The branch is not merged by this story.

## Depends on

`story:harness-parity` (the gaps it finds are closed first) and an llm release after
`story:serving-extraction`.

## Scope (inferred)

harness: `crates/harness-cli`, `crates/harness-app-server`, workspace `Cargo.toml`. Harness's
planning store is `aep.project/1`; this story is held here until it is migrated.
