---
format: aep.planning-md/3
id: review-result:adversary-w29-offline-replay-eval-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:offline-replay-eval, wave 2026-11-24-offline-replay-eval): green, 6 introduced, 0 red cases'
relations:
- reviews: story:offline-replay-eval
revision: 1
---
Recorded by the coordinator as the adversary returned it; nothing is changed. Covers head `f12ba18fd`
of `impl/offline-replay-eval`.

unit: story:offline-replay-eval
verdict: green
cases: executed 199→206, red 0
origin: introduced 6, pre-existing 0, undecided 0
wrote-outside-worktree: scratch `adv2/` only (15 logs, 6 comparison files, `adv2/mutant/` source copy; 30 MB, its `target/` of 1.2 GB removed)
needs-coordinator: no

Covers head f12ba18fd plus my two test files. `git diff --stat`: `crates/llm-calls/tests/replay.rs | 317 +`, `crates/llm-cli/tests/command.rs | 84 +`; 2 files, 401 insertions, both test files. No implementation file touched, no scenario added (conformance not run, `.ess-0.87.0/` never created). No worktree lease taken.

Last pass, one line: I found no defect a caller can reach in the code. What is left is gaps in the tests (findings 1-4) and two document sentences (5, 6).

Order: 7 cases written, run alone (`adv2/new-alone.log`, `new-alone-2.log`: 7 passed), then the suite. `cargo nextest run --locked -p llm-calls -p llm-cli -p llm-decide`: 206 passed, exit 0 (`adv2/suite-final.log`); with my seven deselected: 199 passed (`adv2/suite-before.log`). Slowest test 2.9 s, step 3 s. `cargo fmt --all --check` and clippy `-D warnings` on the three crates pass. The implementor's 209 is from `cargo test`; I did not reconcile the two counts.

All seven cases are green on the real code. Each mutant below passes the unit's 199 tests (`adv2/mutant-suite.log`, `mutant2-suite.log`) and is red on its guard (`adv2/mutant-guards.log`, `mutant2-guards.log`).

**1. The order "uncertain first" is not held.** `crates/llm-calls/src/replay.rs:1069` with `above_limit` tested first: 199 passed. Guard `adversary_w29_a_last_case_known_in_part_and_past_the_cap_says_uncertain_spend` gives `left: (Amount(55000000), Some(LimitExceeded))`. Reaches: `llm replay` on a route whose `price` declares `input` only (the configuration is accepted). The second guard `adversary_w29_replay_on_a_route_priced_for_input_only_says_its_cost_is_not_known` gives `"limit-exceeded"` with a partly known cost. The same two guards are the only tests of the design.md sentence "or the known part of its cost where that is more".

**2. The stderr wording is not held at its boundaries.** `crates/llm-cli/src/lib.rs:529-531` with the cap test first and `>=`: 199 passed. Guards: a run that spent exactly its cap prints `stopped past its cap` (`adversary_w29_replay_that_spent_exactly_its_cap_says_it_stopped_at_its_cap`), and an unknown cost past the cap prints `stopped past its cap (uncertain-spend)`. Reaches: `llm replay` with reserve equal to a case's cost.

**3. Six of the 23 member names can be hidden.** `replay.rs:148-153` without `primary_record_id`, `instructions`, `input`, `label_set.id`, `rubric.text`, `recorded.text`: 199 passed. The refusal then reads `cases[0].*: not an identifier ...`. Guard `adversary_w29_every_member_name_the_format_defines_is_shown_at_its_place` lists all six.

**4. Names below an undefined member or index.** `replay.rs:172` (`unwrap_or(level)`) and `:178` (an index keeps its level): 199 passed. Guard `adversary_w29_no_name_is_shown_below_a_member_or_an_index_the_format_does_not_define` gives `*.cases[0].id.*` and `cases[0][0].id.*`. Only names the format defines can appear, so no caller's text leaks.

**5. Two sentences on `spent` are wrong for a partly known cost.** `CHANGELOG.md:30` "`spent` then holds its reserve" and the guide line 265 "`spent` then counts that reserve". Measured through `llm replay`, reserve 0.00001, route priced for input only: `"spent":"0.000012"`, the known part. `docs/design.md` says it correctly. The guide also names only "the provider reported no usage" as the cause.

**6. Judgement, read not run.** `spec/domains/calls.yaml:71` and `:202` state the stop at the last case; no scenario in `contracts/calls/scenarios/` holds it (floor stays 1068). Only the Rust tests do.

Also added, green, no mutant behind them: `adversary_w29_no_case_is_asked_after_the_one_past_the_cap_or_without_a_known_cost` (first, middle, last; reserve of one nanounit; asked count, `spent`, one skipped record, counts sum to `cases`) and `adversary_w29_replay_takes_a_run_id_that_holds_a_slash`.

Attacked and not broken:
- The 12 pass 1 cases: all present, none ignored, all run. The three re-pinned ones lost only `spent <= money_cap` and the 64-byte rows, as decided, and hold more than before.
- The stop: no later case is asked at any position; what stops the run in the middle is the ledger's own refusal; `spent` matches design.md in all three outcomes.
- Paths: no place found where a name the caller chose is shown (tool arguments, schema `properties`, recorded verdict, unknown member at each level).
- Ids: run id with `/`, rubric id `classify`, case id `classify`, 256-byte case id, digest against readable form. No collision constructed; a multi-byte rubric id is refused as no identifier.
- `Routed` move: the 5 tests pair one to one with `crates/llm-decide/tests/classify/replay.rs`, bodies equal but for the case-id parameters. Nothing names `llm_cli::Routed` or `llm_cli::CLASSIFIER_ID`.
- Read, not run as mutants: the slash refusal, the bound 177, the readable or digest choice and the `above_limit` stop each have a unit test on both sides.

```findings
- file: crates/llm-calls/src/replay.rs
  line: 1069
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: testing above_limit before uncertain_count passes the unit's suite, and a last case with a partly known cost past the cap then reports limit-exceeded
- file: crates/llm-cli/src/lib.rs
  line: 529
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the stderr wording can test the cap before the unknown cost and use >= with the unit's suite green, so a run at exactly its cap or with an unknown cost prints "past its cap"
- file: crates/llm-calls/src/replay.rs
  line: 148
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: six of the 23 member names the format defines can be dropped from the shown list with the unit's suite green, leaving refusals that name only the case
- file: crates/llm-calls/src/replay.rs
  line: 172
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: keeping the level below an undefined member or a second index passes the unit's suite and shows format names where the documents say every member is a star
- file: CHANGELOG.md
  line: 30
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the CHANGELOG and the guide say spent holds the reserve after an unknown cost, while a route priced for input only reports the known part 0.000012 over a reserve of 0.00001
- file: spec/domains/calls.yaml
  line: 71
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the specification states the stop at the last case in comments only and no conformance scenario holds it
```
