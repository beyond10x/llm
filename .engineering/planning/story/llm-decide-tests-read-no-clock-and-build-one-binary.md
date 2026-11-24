---
format: aep.planning-md/3
id: story:llm-decide-tests-read-no-clock-and-build-one-binary
kind: story
status: draft
title: llm-decide tests read no clock and build one integration-test binary
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w28-openai-decisions-client-pass-2
- decomposes: epic:inference
revision: 1
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
