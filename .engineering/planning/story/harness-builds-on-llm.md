---
format: aep.planning-md/3
id: story:harness-builds-on-llm
kind: story
status: draft
title: Harness builds and passes its tests on llm in place of its own wire crates
relations:
- decomposes: epic:serving-split
- depends_on: story:harness-parity
- depends_on: story:serving-extraction
revision: 1
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
