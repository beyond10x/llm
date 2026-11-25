---
format: aep.planning-md/3
id: story:llm-decide-tests-read-no-clock-and-build-one-binary
kind: story
status: active
title: llm-decide tests read no clock and build one integration-test binary
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w28-openai-decisions-client-pass-2
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-decide/src/answer.rs
- confidence: inferred
  path: crates/llm-decide/src/judge.rs
- confidence: inferred
  path: crates/llm-decide/src/schema.rs
- confidence: cited
  path: crates/llm-decide/tests
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:39Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:39Z", actor: "human:operator", revision: 3}
---
## Context

Wave 2026-11-24-rubric-judge added two cases that assert a ratio of wall times, to hold "an answer
is read in time proportional to its length":
`crates/llm-decide/tests/judge.rs` `an_answer_of_any_make_is_read_in_time_proportional_to_its_length`
and `crates/llm-decide/tests/adversary_w27_judge_pass2.rs`
`a_long_answer_of_numbers_is_read_in_time_proportional_to_its_length` (80000 numbers in less than
eight times the time of 20000, the fastest of several runs). That wave's page recorded the risk as a
deviation: the first failure without a defect is a finding against the two cases.

That failure happened in the next wave. The adversary's second pass on
`story:openai-decisions-client` (`review-result:adversary-w28-openai-decisions-client-pass-2`,
finding 4) ran `cargo test -p llm-decide` while other builds ran on the host; the second case
measured 3.8 ms against 53.5 ms and failed. Six repetitions alone measured ratios of 2.9 to 4.4 and
the rerun of the whole command was green. The code under test did not change between the runs.

The same crate builds five integration-test binaries (`adversary_u2_chain`, `adversary_w27_judge`,
`adversary_w27_judge_pass2`, `classify`, `judge`); the rule for this organization's repositories is
one per crate.

## Outcome

The two cases hold the same claim without reading a clock: they count the work the reader does (the
bytes it visits, or the steps of its scanner behind a test-only counter) for two lengths and compare
the counts. A loaded runner cannot fail them. `llm-decide` builds one integration-test binary with
the five files as its modules.

## Acceptance

- No test under `crates/llm-decide/tests/` calls `Instant::now` or compares two durations.
- Each of the two cases fails when the reader is replaced by one whose work grows with the square of
  the length (shown once by a mutant, recorded in the change's message), and passes 50 runs in a row
  while a build runs beside it.
- `cargo test -p llm-decide --no-run` links one integration-test executable; the number of executed
  cases is the same before and after.

## Verification

Targeted: `-p llm-decide` under nextest with the 60 s bound. No live call.

## Scope

Derived 2026-11-25 by `story-scoper` at `main` 831c21d95. Every line is **cited** (read from the
story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-decide/tests/`, the directory the acceptance names — cited
- **Files, clock reads:** `crates/llm-decide/tests/judge.rs`
`an_answer_of_any_make_is_read_in_time_proportional_to_its_length` (:580; `Instant::now` :623 and
:659, `.elapsed()` :625 and :661, `long < short * 8` :637 and :670) and
`crates/llm-decide/tests/adversary_w27_judge_pass2.rs`
`a_long_answer_of_numbers_is_read_in_time_proportional_to_its_length` (:149; `Instant::now` :156,
`.elapsed()` :158, `long < short * 8` :170). There are no more: no other test under
`crates/llm-decide/tests/` names `Instant`, `SystemTime` or `elapsed`
(`grep -rnE 'Instant|SystemTime|elapsed'` finds only these lines and the two imports, `judge.rs:24`
and `adversary_w27_judge_pass2.rs:21`) — cited
- **Files, scripted clocks:** every other `Duration` in the tests comes from a fake
`llm_calls::Clock` (`Scripted` in `classify.rs`, `Stepped` in `judge.rs`, `Micros` in
`adversary_u2_chain.rs`, `Still` in both `adversary_w27_*` files, `Hand` in `classify/replay.rs`).
One case compares such a value with a constant: `adversary_u2_chain.rs:409`, in
`an_answer_before_the_deadline_is_used_on_a_clock_finer_than_a_millisecond`
(`clock.monotonic() < Duration::from_millis(2_000)`). It reads no wall clock, but a literal reading
of "compares two durations" matches it — cited
- **Files, the five binaries:** `crates/llm-decide/tests/adversary_u2_chain.rs` (4 `#[test]`),
`adversary_w27_judge.rs` (8), `adversary_w27_judge_pass2.rs` (4), `judge.rs` (28) and `classify.rs`
(11, plus 42 in its four child modules `classify/endpoint.rs` 10, `classify/adversary_w28_endpoint.rs`
4, `classify/replay.rs` 10, `classify/whole_answer.rs` 18): 97 cases by attribute count, five
binaries, still five at this commit. `crates/llm-decide/Cargo.toml` has no `[[test]]` and no
`autotests` key — cited
- **Files, shared helpers:** none across the five. Each file defines its own `id`, `block_on`,
`EXAMPLE` and fakes and imports from no other test file. The only sharing is inside `classify`:
`classify.rs:1035-1045` declares the four children by `#[path]`, and they import from `super::`
(`classify/endpoint.rs:4`, `classify/adversary_w28_endpoint.rs:4`, `classify/replay.rs:4`,
`classify/whole_answer.rs:7`) — cited
- **Files, new:** `crates/llm-decide/tests/main.rs` with the five files as modules under
`crates/llm-decide/tests/main/`. They cannot stay at the top of `tests/`: cargo builds every
`tests/*.rs` as its own binary unless the manifest sets `autotests = false`. The repository's one
existing single-binary layout is `checks/xtask/tests/gate/main.rs` — inferred
- **Files, paths that break on the move:** `include_str!("../../../examples/catalog.toml")` at
`adversary_u2_chain.rs:27`, `adversary_w27_judge.rs:27`, `adversary_w27_judge_pass2.rs:24`,
`judge.rs:27`, `classify.rs:32`, and the four-level form at `classify/replay.rs:24`: each gains one
`../` per directory level the file moves down — inferred
- **Symbols, the reader measured:** both cases time `llm_decide::judge`
(`crates/llm-decide/src/judge.rs:250`) end to end, through `run_as` (`tests/judge.rs:194`) and
`judged` (`tests/adversary_w27_judge_pass2.rs:103`). On the answer it runs, at
`src/judge.rs:312-314`: `answer::whole` (`src/answer.rs:32`: serde_json driving the `Strict`
visitor, :87-158, then `integers_are_exact`, :162), `schema::holds` (`src/schema.rs:77`) and
`canonical` / `write_canonical` (`src/judge.rs:349`, :355). The defect the cases were written
after sat in `integers_are_exact` (`src/answer.rs:188`; commit 20f06fa6c replaced
`rest.strip_suffix(after)`, which compared the whole remaining text once per number) — cited
- **Also likely:** `crates/llm-decide/src/answer.rs` — inferred, where a counter would sit. An
integration test links the library built without `cfg(test)`, so a `#[cfg(test)]` counter is
reachable only from unit tests in `src/`. With no change to the public API (`mod answer` and
`mod schema` are private, `src/lib.rs:28-33`; `whole` is `pub(crate)`), the counter is a
`#[cfg(test)]` thread-local in `src/answer.rs` and the two cases move into the unit-test module
that file already has (`src/answer.rs:198-199`, 2 cases today). `block_on` in the tests polls on
the calling thread, so a thread-local is read by the case that caused it
- **Also likely:** `crates/llm-decide/src/schema.rs` and `crates/llm-decide/src/judge.rs` —
inferred, only if the count is to cover checking and writing as the `judge.rs` case's own comment
claims ("reading, checking and writing")
- **Documents:** none to change. `AGENTS.md` cites no `llm-decide` test (no row of the invariants
table, nothing in "Gate"); `checks/`, `Taskfile.yml`, `.github/` and `.config/nextest.toml` name
none and carry no filter by binary — cited
- **Documents, history that names the binaries:** `docs/plan/wave-2026-11-24-rubric-judge.md:66`
(`llm-decide::judge an_answer_of_any_make_…`) and :93-94 (the deviation),
`docs/plan/wave-2026-11-24-openai-decisions-client.md:95-96` (`adversary_w27_judge_pass2`), and
seven pages under `.engineering/planning/review-result/`. They record past runs and stay as
written — inferred
- **Confidence:** medium — high for the test side (the story names both cases and all five
binaries, and the tree confirms them); the `src/` side depends on a choice the story leaves open
(bytes visited or scanner steps, and whether the cases stay under `tests/`)
- **Would collide with:** any unit that moves, renames or adds a file under
`crates/llm-decide/tests/`, and any unit that edits `crates/llm-decide/src/answer.rs` (the strict
reader `Strict` and `integers_are_exact`)
- **Overlap with `story:test-binaries-one-per-crate`:** that story moves every crate's test files
into one `tests/main.rs` with modules under `tests/main/`. Both stories touch all nine files
under `crates/llm-decide/tests/` (the five top-level files and the four under `classify/`), create
`crates/llm-decide/tests/main.rs`, and rewrite the six `include_str!` paths and the `#[path]`
lines at `classify.rs:1035-1045`. They do not meet in `AGENTS.md`: that story updates every test
path the invariants table cites, and the table cites none of `llm-decide`. Its count of 242 files
is of 0.25.0; this commit has 247 (246 under `crates/`, 1 under `checks/conformance`), 5 of them
here — cited
- **Safety fact:** the five files share no item and no test reads process state (no environment
variable, file, thread, child process or socket in `crates/llm-decide/tests/`), so turning five
binaries into five modules of one changes no case's input — step 2 (`grep -rnE
'thread::|spawn|std::env|std::process|Command::new|std::fs|TcpListener' crates/llm-decide/tests`
is empty; imports read per file), unproven
