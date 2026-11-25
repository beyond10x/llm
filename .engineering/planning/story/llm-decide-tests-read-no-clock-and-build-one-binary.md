---
format: aep.planning-md/3
id: story:llm-decide-tests-read-no-clock-and-build-one-binary
kind: story
status: implemented
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
- confidence: cited
  path: crates/llm-decide/tests/main
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:39Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:39Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T19:02:19Z", actor: "human:operator", revision: 11, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
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

Read by the coordinator from `git diff --stat` of the integration branch against the wave's opening
commit (`45e7c1c13`) at the close of wave 2026-11-25-tests-without-a-clock: 11 files under
`crates/llm-decide`. The section before this one was derived by `story-scoper` on 2026-11-25.

| Path | Scoped | What the unit did |
| --- | --- | --- |
| `crates/llm-decide/src/answer.rs` | inferred | `integers_are_exact` reads the answer through `scan::Scanner` and holds no other reference to the text; the scanner has `skip_to_token`, `number_literal` and `next_char`, and under `#[cfg(test)]` charges one unit for each char it passes; the `Strict` visitor charges one unit for each element and member. The two cases live here as unit tests and compare counts at two lengths. The answers of the scan do not change |
| `crates/llm-decide/tests/main/main.rs` | cited (the directory) | new: the one integration-test target, five modules |
| `crates/llm-decide/tests/main/judge.rs`, `adversary_w27_judge_pass2.rs` | cited | moved; the two wall-time cases and their helpers are removed |
| `crates/llm-decide/tests/main/classify.rs`, `classify/*.rs`, `adversary_u2_chain.rs`, `adversary_w27_judge.rs` | cited | moved; `mod` and `include` paths follow |

Not touched: `crates/llm-decide/src/judge.rs` and `src/schema.rs` (scoped as inferred), the public
API of the crate, every document, `CHANGELOG.md`.

Limits of the count, written in the doc comments of `mod scan` and `mod work`: work inside one visit
of the visitor, work inside the scanner's methods, the literal of a number read again after the
scanner passed it, a copy of the text built from the chars `next_char` hands out, and what
`serde_json` does between two calls of the visitor.
