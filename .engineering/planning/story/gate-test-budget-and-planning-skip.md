---
format: aep.planning-md/3
id: story:gate-test-budget-and-planning-skip
kind: story
status: draft
title: The gate's test phase is bounded and measured, and a planning-only pull request skips it
summary: nextest with a 60 s kill and a 120 s step bound in CI and locally; planning or prose pull requests skip the Rust gate
relations:
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

The test phase of the gate is its own step and task, run by cargo-nextest under a configuration in
the repository: a test over 60 s is killed and fails, the step fails over 120 s and prints its ten
slowest tests, the same way locally and in CI. Test builds carry no debug information. A pull
request that changes only planning or prose files passes the Gate's jobs without building or
testing.

## Why

Review of 0.24.0 (2026-11-23), finding 15: the Gate's "Rust libraries" step took 8 min 04 s and
combined workspace tests, repeated package tests, checks, formatting and Clippy; `task check` ran
`cargo test` with no 60 s bound per test and no 120 s bound on the step
(`.github/workflows/gate.yml:16`, `Taskfile.yml:23`, run 44872867162). Finding 16: pull request 75
changed one `.engineering/` file and its "Rust libraries" step took 8 min 18 s; the workflow ran
the whole gate for every pull request. Both break the workspace rules "Tests stay fast" and
"Planning-only pull requests skip the tests" of `~/beyond10x/AGENTS.md`. Known slow tests:
`crates/llm-models/tests/routes.rs::no_alias_free_document_within_the_byte_bound_is_refused_by_the_alias_bound`
(15 to 77 s), `crates/llm-responses/tests/adversary2_client.rs` (19 s; two tests 5.6 s and
5.9 s), `crates/llm-models/tests/feature_declarations.rs` (19 s for 3 tests).

## Acceptance

- `task test` and the Gate step "Rust tests" run the same test phase: every package run of the
  former "Rust libraries" step, through `cargo nextest run` under `.config/nextest.toml`, which
  kills a test at 60 s and counts it failed (`slow-timeout` with `terminate-after`) (finding 15).
- The test phase prints its ten slowest tests with their times and its own run time, and fails
  when the runs take longer than 120 s; the binaries are built before the bound starts
  (finding 15).
- The Gate's build checks, formatting and Clippy run in a step apart from the tests; CI installs
  cargo-nextest pinned by version and SHA-256 (finding 15).
- The three named slow tests each finish under 5 s on the developer host and prove what they
  proved before: the same documents, bodies and assertions (finding 15).
- A test build carries no debug information: `readelf -S` on a test binary lists no `.debug_*`
  section, set in `.cargo/config.toml` so the local gate and CI build alike (finding 15).
- A pull request whose changed files are all under `.engineering/` or are `*.md` outside
  `website/` (and are read by no gate step) passes every Gate job, under its existing name,
  without building or testing; any other change, the workflow itself included, runs the whole
  gate, and a change to `website/` still runs Documentation validation. The path list is one list
  in `.github/workflows/gate.yml`, and a test holds an answer on each side of it (finding 16).
- The test phase's measured time is recorded as `test_result` evidence on this story.

## ESS first

None: gate configuration, a repository tool and test performance; no behaviour, noun or contract
changes.
