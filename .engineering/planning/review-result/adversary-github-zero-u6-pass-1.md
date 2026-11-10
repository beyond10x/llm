---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u6-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:durable-shadow-cap, wave 2026-11-09-github-zero)
relations:
- reviews: story:durable-shadow-cap
revision: 1
---
unit: llm u6 story:durable-shadow-cap, branch impl/durable-shadow-cap at 8f27c010 plus my one untracked test file
verdict: CONFIRMED
cases: executed 36→42, red 1
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: 4 paths
needs-coordinator: whether the docs the story scoped (`docs/implementation-status.md:162`, `CHANGELOG.md:19`, `AGENTS.md:57`) are this unit's to update or a later step's

The money cap can be passed: if the clock goes back across UTC midnight between two runs, the durable ledger opens a fresh cap for the earlier day. Separately, the gate never runs the durable tests.

**1. What I touched**

`git --no-pager diff --stat` prints nothing because the only change is a new untracked file. `git status --short` shows it:
```
?? crates/llm-calls/tests/adversary_w2026_11_09_u6.rs
```
Only a test file. No implementation file was changed.

**2. Cases added** (`crates/llm-calls/tests/adversary_w2026_11_09_u6.rs`, whole file behind `#![cfg(feature = "sqlite")]`)

| Case | What it asserts | Now |
|---|---|---|
| `a_restart_with_the_clock_set_back_a_day_does_not_open_a_fresh_cap` | Day D is spent to its cap (a second shadow is skipped with `LimitExceeded`). After a restart with the clock on D-1, the next shadow is skipped with `ClockReversed`, as the in-process lane does in the same test | **red** |
| `a_restart_with_the_clock_set_back_within_the_day_skips_the_shadow` | A restart with the clock before the day's last write gives `Skipped(ClockReversed)` and nothing is sent | green |
| `a_crash_with_a_shadow_in_flight_admits_nothing_more_after_the_restart` | A shadow future is dropped while in flight, then the ledger reopens: `Skipped(UncertainSpend)`, 0.25 still exposed | green |
| `a_changed_cap_or_bound_between_runs_skips_the_shadow` | After a restart, a raised cap or a changed `max_active` gives `Skipped(PolicyMismatch)` and nothing is sent | green |
| `escaped_and_digested_shadow_ids_stay_inside_and_never_share_a_cap` | Ids `.`, `..`, `a/b`, `a%2fb`, `%2e%2e` and two 256-byte ids differing in the last byte give 7 distinct entries inside the directory, 2 of them `%sha256-` | green |
| `a_symlinked_directory_is_the_same_ledger_as_its_target` | The owner lock holds whether the directory is reached through the symlink or the real path, and the day's spend survives a reopen through the other path | green |

Red output, first run of this file on its own (`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-calls --features sqlite --test adversary_w2026_11_09_u6`):
```
test a_restart_with_the_clock_set_back_a_day_does_not_open_a_fresh_cap ... FAILED
thread 'a_restart_with_the_clock_set_back_a_day_does_not_open_a_fresh_cap' (3934361) panicked at crates/llm-calls/tests/adversary_w2026_11_09_u6.rs:356:5:
assertion `left == right` failed: a shadow was sent on a fresh cap for day 20397 after day 20398 was spent to its cap
  left: 3
 right: 2
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.41s
exit 101
```

**3. Suite runs** (after the cases existed)

`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-calls --features sqlite --no-fail-fast` gave exit 101:
```
adversary_u1_attempts 3 passed; adversary_u4_shadow 3 passed;
adversary_w2026_11_09_u6 FAILED. 5 passed; 1 failed (…:356:5 left: 3 right: 2)
record 9 passed; security_u4_cap 3 passed; shadow 18 passed (incl. 4 durable::*); doctests 0
error: 1 target failed: `-p llm-calls --test adversary_w2026_11_09_u6`
```
That is 42 tests run, 36 without my 6 (this is the "before" count).

`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-calls --no-fail-fast` (the default lane) gave exit 0. `shadow` ran 14 tests, and `durable::*` ran 0 times.

**4. Findings** (all cover 8f27c010)

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| `crates/llm-calls/src/shadow.rs:199` | CONFIRMED | introduced | The case above, red at `adversary_w2026_11_09_u6.rs:356`: day D is spent to its cap, then after a restart with the clock on D-1 a new ledger is created and a third shadow goes out. One real day ends up spending up to twice the cap. The guard against an earlier day (`days.latest`, :259) lives only in memory, and :170 promises "a cap is never started afresh". Fix: in `day_ledger`, refuse with `ClockReversed` when `<dir>/<shadow>/` already holds a later day. | The public `ShadowLedger::durable`, which nothing in the repo calls yet. The trigger is the wall clock being stepped back across UTC midnight between two runs, for example by an NTP correction |
| `Taskfile.yml:25` (also `.github/workflows/gate.yml:20`) | CONFIRMED | introduced | The default-lane run executes 0 `durable::*` tests. No workspace member turns on `llm-calls/sqlite`, and neither Taskfile nor gate has `cargo test -p llm-calls --all-features`, so the acceptance tests never run in the gate. Clippy `--all-features` only compiles them | `task rust` and the CI gate on every PR |
| `docs/implementation-status.md:162` | CONFIRMED | introduced | It still says "a durable cap is not built". `CHANGELOG.md:19` and `AGENTS.md:57` still say the cap is only per process. All three were in the story's scope and the unit did not touch them | Readers of the status page and the generated `website/data/status.json` |

**5. Attacked and held**
- Crash between reserve and settle: on reopen the reservation is marked uncertain and nothing more is admitted that day.
- Crash between reserve and begin: the reservation stays held until the day ends.
- Reservation still open at day rollover: it settles on the day it was opened.
- Two ledgers in one process on one directory: the second gets `OwnerBusy` and never sends.
- Two processes racing to create the same day: the directory create is atomic, so the loser is skipped.
- Changed cap or `max_active` between runs: `PolicyMismatch`, skipped.
- Clock back within a day across a restart: `ClockReversed`, skipped.
- Escaping: `..`, `/` and NUL are escaped, the empty id is refused by `Id`, `%xx` names cannot be confused with `%sha256-` names, and long ids get distinct digests.
- Symlinked directory: one owner lock, spend kept.
- A shadow refused at open is never sent; every failure path of `open` returns before the send.
- `ShadowLedger::new` behaves exactly as before: same logic in the diff, and 14/14 shadow and 3/3 security tests pass on the default lane.

**6. Paths written outside the worktree** (the unit's assigned `scratch-adv/` directory)
- `scratch-adv/` (directory)
- `case-run.log`
- `suite-sqlite.log`
- `suite-default.log`

The build output went to the worktree's own `target/`. The lease is released.

```findings
- file: crates/llm-calls/src/shadow.rs
  line: 199
  category: property
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "a restart with the clock set back across UTC midnight opens a fresh durable cap for the earlier day, so one real day can spend twice the cap, contrary to shadow.rs:170"
- file: Taskfile.yml
  line: 25
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "no gate step tests llm-calls with the sqlite feature, so the four durable acceptance tests are compiled by clippy but never executed"
- file: docs/implementation-status.md
  line: 162
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the status page still says a durable shadow cap is not built, and CHANGELOG.md:19 and AGENTS.md:57 still say the cap is only per process"
```
