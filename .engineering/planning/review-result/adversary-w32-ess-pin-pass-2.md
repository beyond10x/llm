---
format: aep.planning-md/3
id: review-result:adversary-w32-ess-pin-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:ess-pin-current-and-local-gate-finds-it, wave 2026-11-25-ess-pin): red, 6 notes, 4 red cases'
relations:
- reviews: story:ess-pin-current-and-local-gate-finds-it
revision: 1
---
## Review

Adversary, pass 2 (the last), of `story:ess-pin-current-and-local-gate-finds-it` at `28381cbc4`
(branch `impl/ess-pin`, correction 2 on pass 1's head `552099b35`), wave 2026-11-25-ess-pin.
Verdict: red. Cases executed 64 → 70, 4 red. Origin: 6 introduced, 0 pre-existing. Every finding is
a note.

Suite after the cases existed (`cargo nextest run --locked -p llm-conformance -p llm-xtask`,
Rust 1.98.0): `70 tests run: 66 passed, 4 failed, 0 skipped`, exit 100. Clippy on 1.98.0 over the
added cases: exit 0.

Pass 1's six findings, each closed:

1. The help flag: `adversary_w32_a_help_flag_as_the_first_argument_reaches_the_pinned_ess` and
   `every_argument_after_ess_reaches_the_pinned_ess_as_written` are green.
2. The guard tests' inputs: `every_form_a_taskfile_command_can_take_is_read`,
   `adversary_w32_an_ess_in_quotes_or_after_another_commands_dashes_is_bare`,
   `the_step_that_installs_ess_is_exempt_once_and_only_where_it_is`,
   `a_version_worded_another_way_is_read_as_an_ess_version_too`.
3. The signal-ended fetch: closed by the words; `adversary_w32_an_interrupted_fetch_is_never_used`
   and `a_leftover_of_an_interrupted_fetch_is_never_read_and_the_release_is_fetched` are green.
4. The unrun branches: the five pass-1 guards are green, and
   `whatever_is_in_the_stored_place_without_an_ess_is_refused_before_any_download` and
   `a_release_another_run_stored_while_path_was_asked_is_looked_at_again` kill the second-look
   mutants.
5. "The pinned digest decides under either": the new words agree with `resolve`.
6. "Before any test runs": the new words agree with the order of the steps in `gate.yml`.

Red cases:

- `pinned_ess::adversary_w32b_the_runners_help_for_ess_lists_no_flag_of_its_own`:
  `llm-conformance help ess` lists `-h, --help` as a flag of the subcommand, while
  `llm-conformance ess --help` goes to the pinned `ess`.
- `ess_pin::adversary_w32b_an_ess_a_step_starts_outside_its_run_is_found`: the workflow reader
  reads a step's `run:` and nothing else; an `ess` in an action's `with:` input, a step's `shell:`
  or a job's `defaults.run.shell` is not counted. No workflow of the repository has such a step.
- `ess_pin::adversary_w32b_an_ess_that_starts_the_line_after_the_runners_words_is_bare`: the
  runner's six words at the end of one line exempt an `ess` that starts the next line. No script of
  the repository is written that way.
- `ess::tests::adversary_w32b_no_run_starts_anything_in_a_fetch_directory`: "No run reads a
  `.fetch-*` directory as stored or starts anything in one" is false for the fetch's own staging
  directory, where the unpacked `ess` is asked its version.

Read in the changed text, not run: the doc comment of `ess_pin.rs` lists `$(printf ess)` as a name
that cannot be counted, and its word is counted; `AGENTS.md` says "every string of `Taskfile.yml`"
without the `desc` and `summary` exception; a comment of `ess.rs` says a move onto what is in the
place fails, and a move onto an empty directory succeeds on Linux.

Green guards added, each red under a mutant the unit's own cases miss:
`pinned_ess::adversary_w32b_an_argument_that_is_not_utf8_reaches_the_pinned_ess_as_written` (a
lossy copy of the arguments), `ess_pin::adversary_w32b_a_tag_and_a_second_joined_name_give_their_versions`
(no `tag` between name and version; only the first `ess-` of a word). A third mutant, the count of
"Install ess" steps over `gate.yml` only, passes every case and changes no outcome today.

Attacked and not broken: the second look removed, or taking the place's `ess` without its version;
the refusal limited to non-empty directories; the Taskfile reader skipping maps under `for`; the
exemption by step name alone; each of the four version wordings dropped; `ess.exe` dropped: each
killed by the unit's own cases. No path reads or renames a `.fetch-*` leftover. A release stored
between the second look and the `rename` is taken by the lost-race branch after its version is
asked. Outside the repository root `llm-conformance ess` ends 1 and names what it needs. All five
workflow files and the whole `Taskfile.yml` are read and green; a new workflow file is read. Not
run: `llm-conformance check`.

```findings
- file: checks/conformance/src/main.rs
  line: 74
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the runner's help for `ess` lists `-h, --help` as its own flag while every flag after `ess` now goes to the pinned ess
- file: checks/xtask/tests/gate/ess_pin.rs
  line: 493
  category: mutant
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the workflow reader reads only `run:`, so an `ess` started through an action's `with:` input or a step's or job's `shell:` passes; no workflow of the repository has such a step
- file: checks/xtask/tests/gate/ess_pin.rs
  line: 277
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the runner's six words at the end of one line exempt an `ess` that starts the next line; no script of the repository is written that way
- file: checks/conformance/src/ess.rs
  line: 246
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the sentence "no run starts anything in a `.fetch-*` directory" is false for the fetch's own, where the unpacked ess is started for its version, in ess.rs and AGENTS.md
- file: checks/conformance/src/main.rs
  line: 101
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a lossy copy of the arguments passes the unit's cases; a guard with an argument that is not UTF-8 is added
- file: checks/xtask/tests/gate/ess_pin.rs
  line: 569
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: three mutants pass the unit's cases (no `tag` between name and version, only the first `ess-` of a word, the step count over gate.yml only); a guard is added for the first two and none can be for the third
```
