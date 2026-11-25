# Wave 2026-11-25-tests-without-a-clock

Status: **delivered** at `becdb9a1d` on `wave/2026-11-25-tests-without-a-clock`; two stories
`implemented`. Wave 9 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-tests-without-a-clock` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Opened on the plan commit
`183126a4d`, which sits on `main` `831c21d95` (LLM 0.32.0 with wave 8 merged), while the release of
0.33.0 and the plan's pull request are in review; the wave takes `main` before its gate. Integration
branch `wave/2026-11-25-tests-without-a-clock`, tree `llm-1125-clk-int`. Skill: AEP implementing
0.26.0.

Consumer gap moved: none. Both stories are test defects found by adversaries of earlier waves: a
case of `llm-decide` that compares wall times failed once under load, and a case of `llm-messages`
that cancels after a fixed 100 ms failed once under load.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `183126a4d`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
  story:llm-decide-tests-read-no-clock-and-build-one-binary (inferred)
wave 2
  story:cli-route-explain (inferred)
  story:messages-cancellation-case-waits-on-a-signal
wave 3
  story:replay-stops-at-the-last-case-have-scenarios (inferred)
wave 4
  story:responses-carries-refusal-text (inferred)
wave 5
  story:test-binaries-one-per-crate (inferred)
wave 6
  story:tool-arguments-with-a-repeated-key (inferred)
6 wave(s), 133 collision(s), 0 unassessed
```

N is two. The verb names no collision between the two stories of this wave.
`story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification`.
`story:docs-ux-layout-checks-from-ux-browser` is wave 7 and touches no file of this wave. The three
properties: implementable in this tree (no credential, no live call), no overlap of files between
the units or with the open wave, and both land before wave 10 moves the files they edit.

The move to `implemented` needs the acceptance evidence. For `llm-decide`: no test under
`crates/llm-decide/tests/` reads a clock; each of the two cases fails on a reader whose work grows
with the square of the length and passes 50 runs in a row beside a build; the crate links one
integration-test executable and executes the same number of cases. For `llm-messages`: the
cancellation case holds no `sleep`, keeps its three assertions, and passes 100 runs beside a build;
the other cases that wait a fixed time are listed, each changed or named with its reason.

## Units

| Unit | Story | Branch | Tree |
| --- | --- | --- | --- |
| 1 | `story:llm-decide-tests-read-no-clock-and-build-one-binary` | `impl/llm-decide-no-clock` at `3839fcbaf`, merged as `becdb9a1d` | `llm-1125-clk-dec` |
| 2 | `story:messages-cancellation-case-waits-on-a-signal` | `impl/messages-cancel-signal` at `0cf610a16`, merged as `46f6d39e2` | `llm-1125-clk-msg` |

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| `llm-decide`: where the two cases live | unit tests beside the reader in `crates/llm-decide/src/answer.rs`; a counter under `#[cfg(test)]` cannot be read from `tests/`, and the public API does not change |
| `llm-decide`: what is counted | the reader's work (`answer::whole` with `integers_are_exact`), as the story's Outcome says |
| `llm-decide`: "the same number of cases" | the number `cargo test -p llm-decide` executes over the whole package |
| `llm-decide`: the layout | `crates/llm-decide/tests/main/main.rs` with the five files as modules beside it, as `checks/xtask/tests/gate/main.rs` |
| `llm-messages`: which signal | the server's signal after it wrote the head and a signal the sink gives when `emit` is entered; the server's alone does not prove the client read the head |
| `llm-messages`: the third acceptance line | the scoper's list goes into the story's close; `the_turn_deadline_bounds_a_sink_that_never_accepts` is changed or named with its reason |
| Documents and CHANGELOG | none: no behaviour a caller sees changes. The wave has no release of its own; its commits ship with the next release that has an entry |

## Pre-flight

- 86 GB free on `/`; the shared compile cache; builds on the workstation's pool.
- Two unit trees and the integration tree, each with its own `target/`.
- No credential, no live call, no network request in either unit.

## Close

The gate on `wave/2026-11-25-tests-without-a-clock` at `becdb9a1d`, Rust 1.98.0 read from
`rust-toolchain.toml`, each step run alone with its own exit status; its compile and test jobs ran
on the build node of the pool:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2442 tests in 5 runs (2127, 129, 23, 80, 83), all passed (in the workspace run one marked leaky by the runner and 1 skipped); the runs took 8.4 s of the 120 s bound; slowest 5.2 s (`llm-bedrock::process_credentials an_expired_answer_makes_every_caller_wait_for_the_one_run`), a test this wave did not touch, as the leaky one (`llm-bedrock::process_credentials a_process_the_command_leaves_holding_its_output_does_not_hold_the_answer`) |
| `task docs` | 0 | the generated pages current, example blocks match |
| `llm-conformance ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` |
| `llm-conformance check` | 0 | `{"total":1090,"passed":1090,"failed":0,"error":0,"unsupported":0,"skipped":0}`; `foundation conformance and deterministic ESS projections passed` |
| `aep plan artifact validate` | 0 | `valid` |

No live call was made.

`story:llm-decide-tests-read-no-clock-and-build-one-binary` is `implemented` on its three
acceptance lines:

- no test under `crates/llm-decide/tests/` calls `Instant::now` or compares two durations: the two
  cases are unit tests in `crates/llm-decide/src/answer.rs` and compare counts of the reader's work
  at two lengths (100004 and 400004 units for 20000 and 80000 numbers);
- each case fails on a reader whose work grows with the square of the length (25092504 and
  400370004 units; 39303129 and 625962504 for the integer list of the second case) and passed 50 of
  50 runs beside a Clippy build;
- `cargo test -p llm-decide` links one integration-test executable where it linked five, and
  executes 99 cases before and after (97 integration and 2 unit cases became 95 and 4).

`story:messages-cancellation-case-waits-on-a-signal` is `implemented` on its three acceptance
lines:

- the cancellation case holds no `sleep` and no bound in time between the start of the turn and the
  cancel: it waits on the server's signal after the head and on the sink's signal when `emit` is
  entered;
- its three assertions are the base's;
- the other cases that wait a fixed time are listed in the story's Scope section, each with its
  reason; `the_turn_deadline_bounds_a_sink_that_never_accepts` is changed.

Counts: `llm-decide` 99 → 99 cases and 5 → 1 integration-test executables; `llm-messages`
`tests/client.rs` 12 → 13 cases. The workspace run links 266 test binaries; wave 10 lowers that
number.

## Decisions after dispatch

| Id | Decision | Why |
| --- | --- | --- |
| D46 | the visitor's constant charge stays; what the count cannot see is written in the doc comment of `mod work` | a charged container for the visitor's own state is a second mechanism beside the scanner; the adversary's mutants that read the array or the keys so far again are named there |
| D49 | the two notes of pass 2 on `skip_while` and on the doc list went back to the implementor as correction 2, read by the coordinator | the scan of before `20f06fa6c`, the reader the two cases exist to refuse, passed both cases through one skip of everything |
| D52 | a scan that copies the chars `next_char` hands it and reads the copy again stays unseen by the count and is written as a limit | the types do not stop a copy; closing it takes `next_char` from the scan |
| D50 | the adversary's case `adversary_w36_a_cancel_wakes_a_turn_parked_in_the_sink` is part of unit 2, and no draft is filed for its finding | the finding is older than the unit, and the case that closes it is 65 lines in the file the unit changes, red on the changed client in 9 ms |
| D48 | `story:rust-toolchain-file-pins-the-gate` (pull request 111, merged as `b7be02632`) is moved to `implemented` by this wave's close | its last acceptance line needs the forge's runs on the pull request, so the move could not ride in that pull request without a second round of the forge |

## Reviews

| Unit | Pass | Covers | Verdict | Cases | Findings | Outcome |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 1 | `ca51d010e` | none by a run | 0 executed | 3 introduced (1 warning, 2 notes), read from the code | `review-result:adversary-w36-llm-decide-no-clock-pass-1`: the two notes fixed in correction 1 (`c2affd308`), the warning written as a limit (D46) |
| 1 | 2 | `c2affd308` | green | 99 → 99, 0 red | 3 introduced notes | `review-result:adversary-w36-llm-decide-no-clock-pass-2`: two fixed in correction 2 (`3839fcbaf`), one is the written limit |
| 2 | 1 | `b8f575e6f` | green | 12 → 13, 0 red | 1 pre-existing note | `review-result:adversary-w36-messages-cancel-signal-pass-1`: the adversary's case taken (`0cf610a16`) |

Unit 1 had two passes, the most the wave rules allow. The coordinator read the diff of correction 2
(`c2affd308..3839fcbaf`, one file): two predicates move into `mod scan`, `skip_while` becomes the
private `pass`, and doc comments change. A copy of the scan of the commit before the unit gives the
same answer on 71 named and 400000 generated texts after it.

What the passes found:

- Unit 1, pass 1: `Scanner::skip_while` returned a slice with the lifetime of the text, so the scan
  could keep slices and read them again uncharged; one row of the second case repeated another; the
  visitor's constant charge cannot see work inside one visit.
- Unit 1, pass 2: eight tries to keep a slice past the next scanner call, each refused by the
  compiler. Eleven mutants: every one whose extra work goes through a call on the scanner is
  caught; the ones that survive work inside one visit, inside a scanner method, on a copy, or on
  the whole text taken through one skip. The last of these was closed by correction 2.
- Unit 2: with the response head 400 ms or 150 ms late the base's cancellation case is red and the
  unit's passes. Of six changed clients five are caught by the unit's cases; a client that sees the
  cancel only when it is polled passes the unit's case and the base's, and is caught by the
  adversary's case.

## Deviations

1. The host restarted while both adversaries ran, and for part of the wave the pool's build node
   was closed to this session's jobs and then opened by the owner's order. Unit 1's first pass
   therefore read the change and built nothing, and unit 2's one pass ran in three rounds.
2. The second acceptance line of the `llm-decide` story says the cases pass 50 runs "while a build
   runs beside it". That was measured at the unit's first two commits (50 of 50 beside a Clippy
   build on the workstation). At its last commit the builds ran on the build node: 50 of 50 runs,
   with no build beside them. The cases read no clock, so load cannot change their counts; the
   adversary's 50 runs at load average 84 gave one distinct set of counts.
3. The adversary of unit 2 measured each changed case 100 of 100 beside four looping copies of the
   test executable. Its runs beside a Clippy build do not count: the build ran on the build node.
   The implementor's 100 of 100 beside a Clippy build on the workstation stand alone.
4. The deadline case of `llm-messages` stays bound by time: it doubles a 300 ms turn limit up to
   9.6 s. A case without wall time needs a deadline the caller can inject in
   `crates/llm-messages/src/`, which no story asks for.
5. The count has limits that the two removed wall-time cases did not have (D46, D52). They are
   written in the doc comments of `mod scan` and `mod work`.
6. The wave took `main` twice before its gate: the plan with release 0.33.0, and the toolchain pin.
   Its close moves the pin's story (D48) and corrects one path in the Scope of
   `story:tool-arguments-with-a-repeated-key` that this wave's move made stale.
7. The opening text of this page names its base as `main` `831c21d95`; the gate ran on `main`
   `b7be02632` merged in.

## Backlog

No draft is filed by this wave. Its one pre-existing finding is closed by the adversary's case
(D50).

## Cost

As the harness reported each agent: tokens are the last total it gave for the agent over all its
rounds, tool uses and seconds are summed over the rounds.

| Agent | Rounds | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| implementor, unit 1 | written without a build, the charged scanner twice, build and test | 217,306 | 83 | 1,629 |
| implementor, unit 2 | written without a build, build and test | 178,437 | 57 | 1,273 |
| adversary, unit 1 | pass 1 read only, pass 2 | 207,997 | 76 | 1,132 |
| implementor, unit 1 | corrections 1 and 2, a new agent | 130,453 | 67 | 1,169 |
| adversary, unit 2 | one pass in three rounds | 194,208 | 72 | 1,572 |

The first runs of both adversaries, which the restart of the host ended, are not in the table: the
harness kept no number for them.
