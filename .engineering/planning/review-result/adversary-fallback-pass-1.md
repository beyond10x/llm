---
format: aep.planning-md/1
id: review-result:adversary-fallback-pass-1
kind: review-result
status: active
title: 'Adversary, ordered fallback, pass 1: one red case on dropped usage evidence, two surviving mutants'
relations:
- reviews: story:ordered-fallback
revision: 1
---
unit: story:ordered-fallback, uncommitted working tree on base `d5c73e8` at `/home/timo/.local/state/worktree/trees/b10x/llm/wave2-fallback`
verdict: CONFIRMED (1 red case, INFEASIBLE by reachability; 2 mutants the suite misses)
cases: executed 23→28, red 1
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: 5 paths (part 6)
needs-coordinator: none

**1. Diff stat**

`git --no-pager diff --stat` is unchanged from the tree I was handed: the same 6 implementor-modified files, 275+/15−. My only path is one new untracked test file, `crates/llm-routing/tests/fallback_adversary.rs`, listed by `git status` as `??`. No implementation file was touched. The mutants ran on a scratch copy of the tree.

**2. Cases added** (`crates/llm-routing/tests/fallback_adversary.rs`)

| case | asserts | now |
|---|---|---|
| `adversary_refused_outcome_keeps_the_attempts_valid_usage_evidence` | a model returns `Ok` with an outcome that `validate_for` refuses, but whose observation is valid (input=7). The attempt is `Failed{Accepted}` and must keep that observation | **red** |
| `adversary_a_cancelled_caller_starts_no_attempt` | a token cancelled before the run gives `Halt::Cancelled` and 0 attempts | green, kills mutant M1 |
| `adversary_cancellation_during_an_attempt_stops_fallback` | the primary cancels, then fails `transport/not-sent`. Expected: `Cancelled`, 1 attempt, secondary never called | green, kills M1 |
| `adversary_too_large_and_unsupported_never_fall_back` | `too-large` / `unsupported` with `rejected` give `IneligibleFailure` | green, kills M2 |
| `adversary_attempt_bound_accepts_the_route_maximum_and_refuses_one_more` | 64 is valid, 65 is refused | green (that mutant is already killed by the existing suite, so no finding) |

The red output, from running the file alone (`cargo test -p b10x-llm-routing --locked --test fallback_adversary`, exit 101):
```
thread 'adversary_refused_outcome_keeps_the_attempts_valid_usage_evidence' (973118) panicked at crates/llm-routing/tests/fallback_adversary.rs:184:18:
accepted attempt's valid bound observation was dropped
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**3. Suite run** (after the cases existed)

Command: `nice -n 19 cargo test -p b10x-llm-routing --locked --no-fail-fast`, exit 101.
```
test result: ok. 11 passed ... (catalog)
test result: ok. 12 passed ... (fallback)
test adversary_refused_outcome_keeps_the_attempts_valid_usage_evidence ... FAILED
test result: FAILED. 4 passed; 1 failed ... (fallback_adversary)
error: 1 target failed:
```
- **The "before" count (23)** is catalog 11 plus fallback 12, the lines this package shows in the implementor's `gate-worktree.log`.
- **Clippy** (`--all-targets -D warnings`) exits 0.
- **Formatting:** `cargo fmt -p b10x-llm-routing --check` passes. I formatted only my own file, with `rustfmt`.

**Mutant probes** (scratch copy, existing `--test fallback` suite):

| mutant | existing suite | my cases |
|---|---|---|
| M1: `fallback.rs:199` becomes `false && cancel.is_cancelled()` | 12 passed, **survives** | 2 fail |
| M2: `fallback.rs:144` adds `ErrorCode::TooLarge` to the eligible codes | 12 passed, **survives** | 1 fails |
| M3: `fallback.rs:47` changes `>` to `>=` | 11 fail, killed | (no finding) |

**4. Findings** (they cover the uncommitted tree on `d5c73e8`)

| # | file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|---|
| F1 | `crates/llm-routing/src/fallback.rs:248` | INFEASIBLE | introduced | On `Ok` + `validate_for` refusal, `failed(&error)` records `observation: None`. The attempt counts as Accepted (billable), yet its valid usage is discarded. The story Context says every attempted route contributes usage/cost evidence. Red at `fallback_adversary.rs:184`. Fix: clone `outcome.observation` into the record when `observation.validate_for(provenance)` passes. | Nothing found. `run_turn` has no production caller. The chat and messages clients validate themselves and return `Err` with the observation (`llm-chat/src/client.rs:98`, `llm-messages/src/decode.rs:133`). Only a third-party `Model` that skips validation reaches it. |
| F2 | `crates/llm-routing/src/fallback.rs:199` | CONFIRMED | introduced | No existing case reaches `Halt::Cancelled`. M1 survives all 12 cases, and `routing-falsification.json` has no cancellation entry. | Any caller that cancels the `Cancel` token, as `llm-core/examples/embedded.rs` does. My two cases now cover it. |
| F3 | `crates/llm-routing/src/fallback.rs:144` | CONFIRMED | introduced | `docs/verification/routing-fallback.md` names `too-large` and `unsupported` as ineligible. The ineligible list in `tests/fallback.rs:418` covers neither, so M2 survives. | Error codes a real projection returns. My case now covers it. |
| F4 | `crates/llm-routing/src/fallback.rs:184` | INFEASIBLE | introduced | Judgement, not tested. A compatible target with no model refuses the whole run, even under `FallbackPolicy::disabled()`. A caller who turned fallback off still has to supply a model for every alternative. This is documented, but it cuts against "callers can disable fallback". | No production caller of `run_turn`. |

**5. Attacked and could not break**
- The order of the pre-attempt guards (attempt bound, then cancel, then deadline, then admit) matches the doc.
- Targets are attempted in `position` order. The catalog sorts them and refuses repeated serving models.
- Every one of the 9 scenario documents matches the code's halt/result paths. I checked this by reading; I did not run the routing scenario lane.
- `select`'s `validate_for` agrees with `capability_rejections`, so no compatible target is refused late.
- Output becoming visible halts both before and after invalid evidence arrives.
- A `not-sent` error that carries an observation becomes `AmbiguousDispatch`.
- Opaque state rejects every target bound elsewhere, the primary included.
- The upper attempt bound (M3 was killed).

**6. Paths written outside the worktree**
- `/home/timo/.cache/llm-wave-2/fallback/adversary-1/mutate.sh`
- `/home/timo/.cache/llm-wave-2/fallback/adversary-1/mutate.log`
- `/home/timo/.cache/llm-wave-2/fallback/adversary-1/red-alone.log`
- `/home/timo/.cache/llm-wave-2/fallback/adversary-1/suite.log`
- `/home/timo/.cache/llm-wave-2/fallback/adversary-1/suite-nff.log`
- Removed already: `adversary-1/copy/` (scratch tree copy) and `adversary-1/target/` (309M).
- The shared build dir `$HOME/.cache/b10x-target/llm-wave2-fallback` received the test binary for my file.

**7. Findings block**
```findings
- file: crates/llm-routing/src/fallback.rs
  line: 248
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: an accepted attempt whose Ok outcome is refused is recorded with observation None, discarding valid bound usage evidence; no in-repo Model reaches this path
- file: crates/llm-routing/src/fallback.rs
  line: 199
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: removing the pre-attempt cancellation check leaves all 12 fallback cases green; Halt::Cancelled is never asserted
- file: crates/llm-routing/src/fallback.rs
  line: 144
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: making too-large eligible for fallback survives the suite although routing-fallback.md names it ineligible
- file: crates/llm-routing/src/fallback.rs
  line: 184
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a missing model for any compatible alternative refuses the run even when the caller disabled fallback with max_attempts 1
```

The brief asked for a JSON array. I used the YAML schema from the charter, which the `aep plan artifact findings` comparison parses.
