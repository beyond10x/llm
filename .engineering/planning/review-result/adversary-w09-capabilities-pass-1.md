---
format: aep.planning-md/3
id: review-result:adversary-w09-capabilities-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:capabilities-declare-streaming, pass 1: nothing found, 7 cases kept'
relations:
- reviews: story:capabilities-declare-streaming
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 1 on `story:capabilities-declare-streaming`, head
`1b851083` (base `787ac412`). The report as returned, paths outside the repository shortened.

```
unit: story:capabilities-declare-streaming, head 1b851083 plus 2 untracked adversary test files
verdict: nothing found
cases: executed 918→925, red 0
origin: introduced 0 / pre-existing 0 / undecided 0
wrote-outside-worktree: 1 directory (the assigned scratch)
needs-coordinator: whether to keep the 7 green adversary cases (2 untracked files) or delete them
```

## Cases added

`crates/llm-models/tests/adversary_w09_capabilities.rs` and
`crates/llm-routing/tests/adversary_w09_capabilities.rs`, all green against the unit:

- 27 YAML spellings of a claim in a mapping route are refused as `invalid-request` and never echo
  a marker value (`True`, `TRUE`, `!!bool`, an alias, `~`, empty, `'true'`, `yes`, `on`, `1`,
  repeated keys both ways, camel and kebab names, `<<` merge, a custom tag).
- 8 spellings of the defaults load at the clients' values.
- A claiming capabilities mapping shared through a YAML alias is refused at its first route, naming
  route `a`.
- 36 combinations of {absent, true, false, null, "true", 1} for the two binding fields: accepted
  exactly when both are at their defaults, and then the revision is `1a6ea40f…` and the output bytes
  match 0.7.0.
- 4 orders of a repeated JSON key are refused.
- 11 catalog TOML forms are refused and never echo the marker.
- For the example catalog, with and without the defaults written out: digest `1365b05f…`, and every
  target's capabilities hold only the 7 keys 0.7.0 reads.

Suite after the cases: `RUSTUP_TOOLCHAIN=1.98.0 cargo test --workspace --locked`, 925 passed,
0 failed, 3 ignored, exit 0. Clippy `-D warnings` on both crates and `cargo fmt --all --check` pass.

## Attacked and not broken

- `ess` 0.60.0 reproduces `contracts/schema` and `contracts/suite.json` (874 scenarios) exactly at
  the head and at the base.
- The `explain` example's output matches `website/docs/guides/explain-a-route.md` field for field;
  the two new fields never appear at their defaults.
- All 17 full literals set both fields; the other sites use `..Capabilities::text`.
- Mutants: removing the check from `bind` or from `Routes::parse`, swapping the setters, or
  inverting either `skip_serializing_if` each turns an existing test red.
- `llm.routes/1` digests have no pinned value at the base; reasoned unchanged, not measured.

```findings
[]
```
