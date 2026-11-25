---
format: aep.planning-md/3
id: review-result:adversary-w32-ess-pin-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:ess-pin-current-and-local-gate-finds-it, wave 2026-11-25-ess-pin): red, 2 warnings and 4 notes, 3 red cases'
relations:
- reviews: story:ess-pin-current-and-local-gate-finds-it
revision: 1
---
## Review

Adversary, pass 1, of `story:ess-pin-current-and-local-gate-finds-it` at `552099b35` (branch
`impl/ess-pin`, base `aa8ba4045`), wave 2026-11-25-ess-pin. Verdict: red. Cases executed 49 → 57,
3 red. Origin: 5 introduced, 1 pre-existing.

Suite after the cases existed (`cargo nextest run --locked -p llm-conformance -p llm-xtask`,
Rust 1.98.0): `57 tests run: 54 passed, 3 failed, 0 skipped`, exit 100; without the adversary's
cases `49 tests run: 49 passed, 8 skipped`, exit 0.

Red cases:

- `pinned_ess::adversary_w32_a_help_flag_as_the_first_argument_reaches_the_pinned_ess`:
  `llm-conformance ess --help` and `ess -h` print the runner's own help and exit 0; the pinned `ess`
  is neither resolved nor started.
- `pinned_ess::adversary_w32_an_interrupted_fetch_leaves_no_download_in_the_store`: a runner ended
  by a signal during the fetch leaves its `.fetch-*` directory in the store.
- `ess_pin::adversary_w32_an_ess_in_quotes_or_after_another_commands_dashes_is_bare`: not counted as
  bare: `sh -c "ess specify validate --path spec"`, `bash -c 'ess …'`, `"ess" specify validate`,
  `env -- ess specify validate`.

Green guards added, each red under a mutant the unit's own cases miss:
`adversary_w32_a_release_another_run_stored_during_the_fetch_is_used`,
`adversary_w32_a_directory_without_an_ess_in_the_stored_place_is_a_refusal`,
`adversary_w32_a_directory_and_a_dangling_link_named_ess_are_passed_over`,
`adversary_w32_a_link_named_ess_to_an_executable_is_taken`,
`adversary_w32_only_the_ess_of_the_asset_is_stored`.

Mutants of the guard tests' inputs that stayed green: a Taskfile command written as a `cmd:` map; a
`sh -c 'ess …'` command; a second step named "Install ess" in another job that runs `ess`; README
text "ESS version 0.87.0 … at v0.87.0".

Attacked and not broken: fifteen mutants of the resolution and the fetch are killed by the unit's
own cases (exit status check, `trim()`, "starts with", stored file unchecked, `PATH` before the
store, fetch over a wrong stored file, staging kept on failure, `SHA256SUMS` check removed or
"contains", byte digest check removed, unpacked version check removed, first `ESS_VERSION` anywhere,
refusal after a projection, projection directory made before the refusal, `ess::run` returning
success). The documented commands were run once: the real fetch passes with the pinned digest;
`ess specify validate` is valid; `ess generate --out target/schema` gives 314 files equal to
`contracts/schema/schema`; `--out contracts/schema` is refused as an unowned output destination; the
synthesized suite is byte-identical. `llm-conformance run` starts no `ess`; `Pin::read` refuses a
renamed step and an upper-case, short or spaced digest, naming the file; a hostile
`LLM_ESS_RELEASES` cannot get bytes of another digest started; a move to an invented 0.99.2 has no
site the pin-move rule does not name. Not checked: that `cargo metadata` without `--locked` re-locks
the `ess-*` packages and nothing else.

Clippy did not run on the added cases: the pool's sync to its build node failed twice.

```findings
- file: checks/conformance/src/main.rs
  line: 73
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a help flag as the first argument of `llm-conformance ess` is answered by the runner and never reaches the pinned ess
- file: checks/xtask/tests/gate/ess_pin.rs
  line: 178
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the bare-ess and one-version tests stay green on a `cmd:` map command, a quoted or `sh -c` ess, a second step named "Install ess" and a version worded "ESS version 0.87.0"
- file: checks/conformance/src/ess.rs
  line: 238
  category: concurrency
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a fetch ended by a signal leaves its `.fetch-*` directory in the store, against "removed again whatever the outcome"
- file: checks/conformance/src/ess.rs
  line: 297
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the lost-race branch, the `is_file` and link handling of `on_path` and the tar member name are run by no case of the unit; five mutants pass its suite
- file: AGENTS.md
  line: 154
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the sentence "the pinned digest decides under either" is false for `LLM_ESS_STORE`, where a file is started on its version answer alone
- file: AGENTS.md
  line: 165
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: the "Install ess" step does not fail before any test runs, because the step "Rust tests" precedes it in gate.yml
```
