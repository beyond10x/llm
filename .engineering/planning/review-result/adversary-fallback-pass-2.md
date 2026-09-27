---
format: aep.planning-md/2
id: review-result:adversary-fallback-pass-2
kind: review-result
status: active
title: 'Adversary, ordered fallback, pass 2: three surviving mutants now guarded by green cases, one judgement on dispatch classification'
relations:
- reviews: story:ordered-fallback
revision: 1
---
unit: story:ordered-fallback, second and last pass, on the uncommitted working tree at base `d5c73e8` (`home-path:sha256:dbeab428e61fbbf75927badff457a86620c369c7f5190943d59e233d935bc12c`)
verdict: CONFIRMED (3 mutants survive all 21 existing fallback cases; no case is red against the tree)
cases: executed 32→35, red 0 against the tree (each new case is red against its mutant)
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: 8 paths (part 6)
needs-coordinator: none

**1. Diff stat**

`git --no-pager diff --stat` is the same as the tree I was handed: 6 files, 385+/15−, all the implementor's or the coordinator's. My only path is one new untracked test file, `crates/llm-routing/tests/fallback_adversary_2.rs`. No implementation file was touched; every mutant ran on a scratch copy.

**2. Cases added** (`crates/llm-routing/tests/fallback_adversary_2.rs`). All three are green on the tree. I wrote them first, then ran the file alone (3 passed, exit 0). They are red only against the mutant each one targets:

| case | asserts | killed mutant, red output verbatim |
|---|---|---|
| `adversary_refused_outcome_never_records_foreign_evidence` | a refused `Ok` outcome whose observation belongs to another binding records no observation | M-a, `fallback.rs:260` `if` → `if true \|\|`: `panicked at …fallback_adversary_2.rs:179:9: assertion left == right failed: run result carries foreign evidence  left: Some(TurnObservation { binding: Provenance { … endpoint: Id("remote-models") … }) right: None` |
| `adversary_foreign_evidence_after_visible_output_is_never_recorded` | after one visible event, a `rejected` failure with foreign evidence records no observation | M-c, the `visible_events > 0` branch (`:279`) moved ahead of the contradiction branch (`:267`): same panic at `:179:9`, foreign observation present |
| `adversary_an_incompatible_trailing_target_is_not_cut_by_the_bound` | with `FallbackPolicy::disabled()` and the only alternative rejected (opaque state), the halt is `Exhausted` | M-j, the bound check (`:185`) moved ahead of the rejection skip (`:182`): `panicked at …:248:5: halt claims a cut the bound never made  left: AttemptBound right: Exhausted` |

The existing suite under each mutant: `fallback` 16 passed, `fallback_adversary` 5 passed. A control mutant, M-k (Accepted → Unknown at `:258`), is already killed by the pass-1 case, so it is not a finding. The full log is `adversary-2/mutate.log`.

**3. Suite run** (after the cases existed)

`nice -n 19 cargo test -p b10x-llm-routing --locked --no-fail-fast` exits 0: catalog 11, fallback 16, fallback_adversary 5, fallback_adversary_2 3. The before count of 32 comes from the implementor's `gate-worktree-2.log`.

- **Clippy:** `--all-targets -D warnings` exits 0.
- **Formatting:** `cargo fmt --check` exits 0, after I ran `rustfmt` on my own file only. It re-wrapped two assert lines after the runs; no behaviour changed.

**4. Findings** (they cover the uncommitted tree on `d5c73e8`)

| # | file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|---|
| A | `crates/llm-routing/src/fallback.rs:260` | INFEASIBLE | introduced | The guard added by F1 has no case. With it forced true, all 21 cases stay green, although `routing-fallback.md` says "Foreign evidence is never recorded." | Only a `Model` returning `Ok` with a foreign observation. No in-repo model does that, and `run_turn` has no production caller. |
| B | `fallback.rs:267` / `:279` | INFEASIBLE | introduced | Nothing tests the branch order that makes foreign evidence be dropped after visible output. Reordering survives. | Only a model that emits output and then fails with foreign evidence. None found in the repo. |
| C | `fallback.rs:182` / `:185` | CONFIRMED | introduced | F4's claim is that `AttemptBound` is set "when the list was cut at the bound". Checking the bound before the skip mislabels an exhausted run, and every case plus the ESS scenario stays green. Fix: none needed in the code; the case now guards it. | Ordinary caller input: `FallbackPolicy::disabled()` with a request whose opaque state rejects the alternatives. |
| D | `fallback.rs:258` vs `:277` | INFEASIBLE | introduced | Judgement, not tested. The two paths classify contradictory evidence differently. A refused `Ok` outcome with foreign evidence records `Dispatch::Accepted`, while an `Err` with foreign evidence records `Dispatch::Unknown`. Both halt, so neither is replayed. | Same third-party-model-only state as A. |

**5. Attacked and could not break**
- The F1 kept/dropped rules: the NotSent-with-bound-evidence path becomes `AmbiguousDispatch` and keeps its evidence.
- F4: only the first `max_attempts` compatible targets need a model, under both the route-level and the caller-level disable.
- The 14 falsification entries: every `before` string occurs exactly once in `fallback.rs`, and every `source_before_sha256` equals the current file hash.
- The Ok-refused path never falls back, and it records `Accepted` (M-k is killed).
- The adapter's use of the eligible-code and dispatch matrix (`Accepted`, `Unknown`) matches `routing-fallback.md`.

**6. Paths written outside the worktree**
- `home-path:sha256:379407403a0aa6775e1404dccff9cdd418440759d17b943ec32b85e1bb53c127` containing `mutate.sh`, `apply.py`, `fallback.rs.orig`, `mutate.log`, `alone-tree.log`, `suite.log` and `clippy.log`.
- Removed already: `adversary-2/copy/` and `adversary-2/target/`.
- The shared build dir `home-path:sha256:150a02889bdd024f401141a7cde92a06454cb2c745ad840f4a6192473b2e4b47` received the `fallback_adversary_2` test binary.

**7. Findings block**
```findings
- file: crates/llm-routing/src/fallback.rs
  line: 260
  category: mutant
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: forcing the F1 bound-evidence guard true records foreign evidence on a refused Ok outcome and all 21 existing fallback cases stay green
- file: crates/llm-routing/src/fallback.rs
  line: 267
  category: mutant
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: moving the visible-output branch ahead of the contradictory-evidence branch records foreign evidence after visible output and no existing case fails
- file: crates/llm-routing/src/fallback.rs
  line: 185
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: checking the attempt bound before the rejection skip reports AttemptBound for a disabled-fallback run whose alternatives were all incompatible, and no existing case or scenario fails
- file: crates/llm-routing/src/fallback.rs
  line: 258
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: foreign evidence on a refused Ok outcome is recorded as Dispatch::Accepted while the same contradiction on an Err path is recorded as Dispatch::Unknown
```
