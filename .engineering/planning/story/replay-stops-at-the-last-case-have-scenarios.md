---
format: aep.planning-md/3
id: story:replay-stops-at-the-last-case-have-scenarios
kind: story
status: active
title: The replay stops at its last case are held by conformance scenarios
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w29-offline-replay-eval-pass-2
- decomposes: epic:routing
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: contracts/baseline.json
- confidence: cited
  path: contracts/calls/scenarios/a-replay-whose-last-case-has-no-known-cost-reports-uncertain-spend.yaml
- confidence: cited
  path: contracts/calls/scenarios/a-replay-whose-last-case-passes-the-cap-reports-limit-exceeded.yaml
- confidence: cited
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/suite.json
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:41Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:41Z", actor: "human:operator", revision: 3}
---
## Context

Wave 2026-11-24-offline-replay-eval added `replay` to `llm-calls`. Its correction round made a run
stop at the case that takes the spend past the cap, and at a case whose cost is not known, also when
that case is the last one of the corpus (`stopped` is `limit-exceeded` or `uncertain-spend`, and
`llm replay` exits 3).

`spec/domains/calls.yaml` states both stops in comments (lines 71 and 202 at the wave's close). No
scenario in `contracts/calls/scenarios/` holds them: the six scenarios of
`story:offline-replay-eval` cover the comparison, the cap at admission, a failed call, the digest,
the role and the label comparison. Only Rust tests hold the two stops
(`review-result:adversary-w29-offline-replay-eval-pass-2`, finding 6).

## Outcome

The two stops at the last case are behaviour the specification states and the conformance check
answers: a run whose last case takes the spend past its cap reports `limit-exceeded`, and a run
whose last case has no known cost reports `uncertain-spend`, each with every case sent and none
`not-sent`.

## Acceptance

- `contracts/calls/scenarios/` holds two scenarios, listed in `contracts/ess-inputs.yaml`:
  `a-replay-whose-last-case-passes-the-cap-reports-limit-exceeded` and
  `a-replay-whose-last-case-has-no-known-cost-reports-uncertain-spend`.
- `llm-conformance check` answers both through the real `replay`; `contracts/baseline.json` rises by
  the number of scenarios now answered and skips stay 0.
- Each scenario fails when the stop is read only on a later case's refusal (shown once by a mutant,
  recorded in the change's message).

## Verification

Targeted: `ess specify validate --path spec` and `llm-conformance check` with the pinned `ess`. No
live call.

## Scope

Derived 2026-11-25 by `story-scoper`, read at `main` 831c21d95. Every line is **cited** (read from the story or the tree; the source is named in brackets) or **inferred** (a reading that could be wrong).

- **Primary surface:** `contracts/`: two new files in `contracts/calls/scenarios/`, two lines in `contracts/ess-inputs.yaml`, the regenerated `contracts/suite.json` and the floor in `contracts/baseline.json` — cited (story, tree)
- No Rust source, no `tests/*.rs` file, no file under `spec/` and no file under `contracts/schema/` has to change — inferred
- **Files, the two stops in code (read, not changed):**
- `crates/llm-calls/src/replay.rs:1053-1097` `summary` decides the stop of a run that sent every case, at `:1069-1075`: `refused.or(…)`, then `totals.uncertain_count != 0` gives `BudgetError::UncertainSpend` (`:1069-1070`), else `totals.above_limit` gives `BudgetError::LimitExceeded` (`:1071-1072`). The unknown cost is tested before the cap — cited (tree)
- `crates/llm-calls/src/replay.rs:914-996` `replay`: `admit` at `:962`; a later case's refusal sets `stopped = Some(cause)` at `:984` (`:972-986`), and every case after it is `NotSent` at `:944-945`. This is the only place the stop was read before the correction round — cited (tree)
- The two ledger flags: `crates/llm-cost/src/budget/engine.rs:369` (`uncertain_count`) and `:371` (`above_limit: exposure > limit`). What leaves a cost unknown: `Spend::record` at `replay.rs:829-848` (`:838` an incomplete cost, `:843` a sent record with no cost) and the settlement at `:1025-1039` — cited (tree)
- `crates/llm-cli/src/lib.rs:519-548` `printed`: exit 3 and the stderr wording (`:528-535`). The conformance target calls the library, not the `llm` binary, so the exit status stays held by Rust tests alone — cited (tree)
- **Files, the Rust tests that hold the stop at the last case today:**
- `crates/llm-calls/tests/replay.rs::a_run_stops_at_its_last_case_when_that_case_takes_the_spend_past_the_cap` (`:1550`) — cited (tree)
- `crates/llm-calls/tests/replay.rs::spent_holds_a_reserve_only_when_the_run_stopped_with_uncertain_spend` (`:1579`) — cited (tree)
- `crates/llm-calls/tests/replay.rs::adversary_w29_a_run_whose_last_case_has_no_known_cost_reports_uncertain_spend` (`:1666`) — cited (tree)
- `crates/llm-calls/tests/replay.rs::adversary_w29_a_last_case_known_in_part_and_past_the_cap_says_uncertain_spend` (`:1935`), which holds the order of the two causes — cited (tree)
- `crates/llm-cli/tests/command.rs::replay_exits_nonzero_when_its_last_case_takes_the_spend_past_the_cap` (`:1589`), `::adversary_w29_replay_of_one_case_whose_cost_is_not_known_says_uncertain_spend` (`:1808`) and `::adversary_w29_replay_on_a_route_priced_for_input_only_says_its_cost_is_not_known` (`:2042`) — cited (tree)
- **Files, how the conformance target answers a replay scenario:**
- `checks/conformance/src/target.rs:156` `execute_command` hands the command to `crate::calls::observe` (`:175`); `checks/conformance/src/calls.rs:51-57` `observe`, `:97-112` `replay`, `:136-219` `replay_runs`, which calls the real `llm_calls::replay` (`:196`) through `llm_decide::Routed` (`:164`) over `Sequenced` scripted models (`:222-258`) — cited (tree)
- A scenario's `timeline` step is command `llm.calls.Replay`, outcome `observed`, with five string inputs (`ReplayInput`, `calls.rs:59-67`): `catalog_toml`, `corpus_json`, `prices_toml`, `run_json` (`RunInput` `:69-78`: `id`, `candidate_route_id`, `ledger_id`, `currency`, `money_cap`, `reserve`) and `program_json` (`ReplayProgram` `:80-92`: `start_unix_ms`, `models` with one `Script` per call in order, `then` for further run ids) — cited (tree)
- `Script` (`calls.rs:416-441`): `answer`, `tool_arguments`, `first_event_after_ms`, `ends_after_ms`, `fail`, `fail_times`, `usage` ("absent reports none", `:436-438`), `upstream_model` — cited (tree)
- A scenario's `assert` reads view `llm.calls.LastCall` with `counts` and `contains` over `valid_program`, `error_code`, `records` (JSON strings), `case_results` (`case-id:result[:code]`, `calls.rs:114-122`) and `replay_runs` (the run as one JSON string without `results`, `:125-132`; it carries `stopped`, `spent`, `same` and `not_sent`) — cited (tree)
- Both stops can be written with these shapes. No change to `spec/domains/calls.yaml`: entity `llm.calls.ReplayRun` (field `stopped`, `:204`), command `llm.calls.Replay` (`:261-277`) and view `llm.calls.LastCall` (`:282-285`) stay as they are, and so does `checks/conformance/src/calls.rs` — inferred
- "The last case passes the cap" is a `run_json` whose `reserve` is below one call's cost. With the fixture prices of the existing replay scenarios a call of 10 input and 5 output tokens costs 0.055; with `money_cap` `0.06`, `reserve` `0.005` and two cases the second is admitted at 0.06 and settles the spend at 0.11: `"not_sent":0`, `"stopped":"limit-exceeded"` — inferred
- "The last case has no known cost" is a last `Script` without `usage`, or without its `output_tokens` (the form `contracts/calls/scenarios/a-record-keeps-unknown-usage-unknown-and-its-cost-incomplete.yaml:118` uses for `llm.calls.Call`): `"not_sent":0`, `"stopped":"uncertain-spend"`, `spent` the earlier costs plus the reserve held — inferred
- The specification states both stops already: `spec/domains/calls.yaml:70-73` and `:201-203` (the story's lines 71 and 202) — cited (story, tree)
- **Files, the existing scenarios:** `contracts/calls/scenarios/` holds 18 files; six drive `llm.calls.Replay`: `a-replay-compares-each-case-with-its-recorded-answer`, `a-replay-stops-at-its-money-cap-and-counts-the-cases-not-sent`, `a-failed-candidate-call-counts-failed-not-differs`, `two-replays-of-one-corpus-report-the-same-digest-and-counts`, `replay-records-carry-role-replay`, `a-classification-case-compares-labels-not-confidences` — cited (story, tree)
- The two closest: `a-replay-stops-at-its-money-cap-and-counts-the-cases-not-sent.yaml` (143 lines, `limit-exceeded` with `"not_sent":2`) for the cap, and `a-failed-candidate-call-counts-failed-not-differs.yaml` (140 lines, `uncertain-spend` with `"not_sent":1`) for the unknown cost. Their catalog, price book and `run_json` can be copied — cited (tree)
- Every existing scenario that asserts a stop has `not_sent` of 1 or more (`replay-records-carry-role-replay` is the third), so none of the 18 fails when the stop is read only on a refusal — cited (tree)
- **Files that move when the two scenarios are added:**
- New: `contracts/calls/scenarios/a-replay-whose-last-case-passes-the-cap-reports-limit-exceeded.yaml` and `contracts/calls/scenarios/a-replay-whose-last-case-has-no-known-cost-reports-uncertain-spend.yaml` — cited (story)
- `contracts/ess-inputs.yaml`: `format: ess-inputs/1`, `specification: []`, then `scenarios:`, one flat list of 1046 paths relative to `contracts/`, in authoring order and not sorted. The `calls` block is `:979-996`, its replay lines `:991-996`; the two new lines go there. `checks/conformance/src/gate.rs:84-112` (`every_authored_scenario_is_declared`) refuses a file the list does not name — cited (story, tree)
- `contracts/suite.json`: generated (4,992,232 bytes, 138,864 lines; 1090 scenarios, 1046 authored and 44 generated), regenerated with the command at `AGENTS.md:195`; `gate.rs:140` fails on drift. Expected hunks: `coverage.authored` (near `:198`), `coverage.authored_sources` (near `:1865`), `coverage.counts.authored` 1046 to 1048 (`:6283`) and two entries under `scenarios` (after `:32526`) — cited (tree)
- `contracts/baseline.json:2-3`: `answered_floor` and `total_floor` 1090 to 1092; `skipped_ceiling` stays 0. Compared at `checks/conformance/src/main.rs:163-165` — cited (story, tree)
- `contracts/schema/`: no file moves, because it is generated from `spec/` alone (`AGENTS.md:196`, `gate.rs:143-149`) — inferred
- **Documents:**
- `AGENTS.md:80`, "The conformance suite answers at least 1090 scenarios and skips none": 1090 to 1092. It is the only line of `AGENTS.md`, `README.md`, `docs/` (wave records under `docs/plan/` aside) and `website/` that states the current floor — cited (tree)
- `README.md` states no floor and no scenario count — cited (tree)
- `docs/implementation-status.md:173` (row `offline-replay-eval`) states both stops at the last case and no scenario count; `:75` says "at least 956", an older bound that stays true. `website/data/status.json` and `website/docs/status.mdx` are generated from that page and move only if it is edited. `docs/design.md:807-820` and `website/docs/guides/use-the-llm-command.md:278` state the behaviour and need no change — cited (tree)
- `CHANGELOG.md`: one line under `## [Unreleased]`, under a new `### Added` heading above `### Fixed` (`:9`), in the form of `:257` (0.19.0) and `:79` (0.29.0): ``- ESS: two `llm.calls` scenarios hold the replay's stops at its last case; the floor is 1092 (was 1090)``. The two numbers follow the merge order — inferred
- **Also likely:** the mutant the acceptance asks for replaces `crates/llm-calls/src/replay.rs:1069-1075` with `refused` alone. It is built in a managed worktree (`AGENTS.md:249-252`), recorded in the change's message and not committed — inferred
- A row for the two stops in the invariants table of `AGENTS.md` (no row names the replay today) is not in the acceptance; if one is added it cites `crates/llm-calls/tests/replay.rs` paths — inferred
- **Confidence:** high for where the work lands, because the story names both files, the manifest and the baseline and every line above was read in the tree; medium for "no target and no spec change", which was walked by reading and not run — inferred
- **Would collide with:** any unit that adds or removes an authored scenario (it regenerates `contracts/suite.json` and moves `contracts/baseline.json:2-3`, `AGENTS.md:80` and a "the floor is N (was M)" line under Unreleased in `CHANGELOG.md`); any unit that edits `spec/` (it regenerates `contracts/suite.json`); any unit that edits the invariants table of `AGENTS.md` at `:79-81`; any unit that changes `Script`, `ReplayProgram` or `run_summary` in `checks/conformance/src/calls.rs`, or `summary` in `crates/llm-calls/src/replay.rs` — inferred
- **Overlap with `story:test-binaries-one-per-crate`:** no test file in common, because this story adds and edits no `tests/*.rs`. Both touch `AGENTS.md` and the Unreleased block of `CHANGELOG.md`. In `AGENTS.md` this story changes `:80`, which cites no test, between `:79` and `:81`, which both cite `checks/conformance/tests/client_turns.rs`; if that story renames that file the three adjacent lines conflict. The seven tests named above move with that story (`crates/llm-calls/tests`, `crates/llm-cli/tests`, its part 5); this story's commit names none of them unless a table row is added — inferred
- **Overlap with `story:tool-arguments-with-a-repeated-key` and `story:responses-carries-refusal-text`:** neither body names a scenario, a floor or a path under `contracts/` — cited (their story files)
- If either adds scenarios, all three touch: `contracts/suite.json` (generated; `coverage.counts.authored` is one shared line, so it always conflicts and is regenerated after the merge, never merged by hand), `contracts/baseline.json:2-3` (the same two lines), `contracts/ess-inputs.yaml` (other domain blocks, clean unless two units append at the same place), `AGENTS.md:80` (the same line) and `CHANGELOG.md` (Unreleased; each floor pair depends on the order of merge). A `spec/` change in either also regenerates `contracts/schema/`, which this story does not touch — inferred
- `story:cli-route-explain` names no scenario; `story:llm-decide-tests-read-no-clock-and-build-one-binary` and `story:messages-cancellation-case-waits-on-a-signal` scope to test files only and share no file with this story — cited (their story files)
- **Safety fact:** the change is data only: the target already reads every member the two scenarios use (`usage` optional at `calls.rs:436-438`, `money_cap` and `reserve` at `:76-77`, `stopped` and `not_sent` kept by `run_summary` `:125-132`), and no existing scenario file changes, so no answer of the 1090 moves. Step 3, walked by reading `replay.rs:943-995` and `:1069-1075` with `calls.rs:136-219`; unproven, nothing was run — inferred
- **Safety fact, second:** the mutant acceptance holds only if each new scenario asserts `"not_sent":0` inside its `replay_runs` string. With `summary` reading `refused` alone, `stopped` serializes as `null` and that string differs; a scenario with a case not sent passes under the mutant, as the 18 existing ones do. Step 3, unproven — inferred
