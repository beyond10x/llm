---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u5-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:codex-renewal-blocking-io-bound, wave 2026-11-09-github-zero)
relations:
- reviews: story:codex-renewal-blocking-io-bound
revision: 1
---
unit: llm unit 5, `story:codex-renewal-blocking-io-bound`, branch `impl/codex-renewal-blocking-io-bound` at `77690c68` plus one untracked test file
verdict: green
cases: executed 79→80, red 0
origin: introduced 1, pre-existing 2, undecided 0
wrote-outside-worktree: 7 paths, all in the unit's scratch-adv directory
needs-coordinator: none

The fix does what the amended acceptance asks: a FIFO at the path during the re-read is refused without blocking. No red case was found against the tree. The one new case passes on the tree and fails only against a mutated copy, where it catches a size-limit mistake the existing suite misses.

**1. Diff stat**

The only change is a new untracked test file:
```
?? crates/llm-credentials/tests/adversary_w2026_11_09_u5.rs
```

**2. Case added**

`crates/llm-credentials/tests/adversary_w2026_11_09_u5.rs::a_login_of_exactly_the_size_limit_is_renewed_and_written_back`. It renews a due login of exactly `MAX_SECRET_BYTES` (1 MiB) and asserts `Renewal::Renewed`, the new access and refresh tokens, and that the padding survives.
- Run alone on the tree: green (`1 passed; 0 failed`, exit=0).
- Run against a scratch copy with `renewal.rs:836` changed from `<=` to `<`: red; every other test in the crate stayed green.
  ```
  panicked at crates/llm-credentials/tests/adversary_w2026_11_09_u5.rs:124:29:
  a login at the size limit was refused: <scratch>/mutant-target/tmp/.tmpSAMGPI/auth.json changed while its renewal request was in flight, so the newer login was not overwritten
  test result: FAILED. 0 passed; 1 failed
  ```

**3. Suite run (after the case existed)**

`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-credentials --features codex-renewal --no-fail-fast`: 80 passed, 0 failed, exit=0, including `write_back_reread_of_a_fifo_is_refused_without_blocking ... ok`. The "before" count of 79 is this run minus the new case.

**4. Findings**

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| `crates/llm-credentials/src/codex/renewal.rs:836` | CONFIRMED | introduced | The new re-read size limit had no test: `<=` to `<` leaves the suite green; the added case goes red. | Only a login of exactly 1 MiB. |
| `crates/llm-credentials/src/codex/renewal.rs:828` | INFEASIBLE | pre-existing | The re-read opens without `O_NOFOLLOW` and does not match device and inode against the `lstat` at :855, so a symlink swapped in between is followed; a link to a FIFO is still refused by `fstat`, a link to a file on a hung mount blocks. The base `fs::read` and the first read (`codex.rs:262-267`) also follow links. | Only a swap in that gap; no test can trigger it reliably. |
| `crates/llm-credentials/src/codex/renewal.rs:888` | INFEASIBLE | pre-existing | Temp-file create (:846), `write_all`/`sync_all` (:850-852), `lstat` (:855), rename (:880) and the blocking open of the directory (:888) still run unbounded under `spawn_blocking` (`local.rs:70`). The story Outcome's bound is not met beyond the narrowed acceptance. | A hung filesystem or a swap race. |

**5. Attacked and held**
- FIFO, character device or directory at the path during the re-read: no block, `fstat` refuses all three.
- File growing or shrinking between `fstat` and read: `read_exact` end-of-file check refuses it as `ChangedDuringRenewal`.
- The comparison after the re-read is still exact against the first read.
- Every new error path gives `ChangedDuringRenewal`, mapped to `RefreshUncertain`, as the base `fs::read` failure was.
- `cargo check --target x86_64-pc-windows-gnu --tests --features codex-renewal` exits 0.
- The unit's FIFO test can fail: removing the non-blocking open makes it time out; removing the regular-file check makes the read return empty bytes.

```findings
- file: crates/llm-credentials/src/codex/renewal.rs
  line: 836
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the re-read size limit is untested: changing <= to < keeps the suite green, and the added case adversary_w2026_11_09_u5.rs catches it"
- file: crates/llm-credentials/src/codex/renewal.rs
  line: 828
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: the re-read follows a symlink swapped in after the lstat at :855 (no O_NOFOLLOW, no device and inode match), so a link to a file on a hung mount still blocks
- file: crates/llm-credentials/src/codex/renewal.rs
  line: 888
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: the temp-file write, sync_all, lstat, rename and the plain blocking open of the directory all still run unbounded under spawn_blocking, so the story Outcome's bound is not met beyond the narrowed acceptance
```
