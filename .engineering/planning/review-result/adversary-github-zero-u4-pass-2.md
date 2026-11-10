---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u4-pass-2
kind: review-result
status: active
title: Adversary, pass 2 (story:call-tool-spawned-refusal-keeps-its-class, wave 2026-11-09-github-zero)
relations:
- reviews: story:call-tool-spawned-refusal-keeps-its-class
revision: 1
---
unit: llm u4 `story:call-tool-spawned-refusal-keeps-its-class`, commit `068405b4` plus one added case in the working tree
verdict: red
cases: executed 22→23, red 1
origin: introduced 1, pre-existing 0, undecided 0
wrote-outside-worktree: 3 paths, all in the assigned scratch-adv2
needs-coordinator: whether the shared-slot leak should hold the unit. The leak only shows when one process runs a direct turn and a concurrent `call_tool` on the same model id, and no process in the repo does both. Pass-1 finding 2 had the same reach and you held the unit on it.

Your provenance question, answered: two models on two different `auth.json` files with the same model id and URL share one slot. A `call_tool` on login A that fails for an unrelated reason comes back as login B's missing-login error, with B's file path in the message.

**1. Diff stat** (one test file only; no implementation or non-test path touched)
```
 .../tests/adversary_w2026_11_09_u4.rs              | 52 ++++++++++++++++++++++
 1 file changed, 52 insertions(+)
```

**2. Case added**
- `crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs:307` `another_logins_refusal_is_not_this_calls_failure`, appended; no existing case edited. It builds two Codex models with the same model id and URL: A has a valid login, B's login is absent. The case first asserts that their provenances are equal. A's `call_tool` is sent and the server holds it. Meanwhile B runs `turn` directly, outside `call_tool`, the way control-plane's `ProviderPort::turn` drives `codex_model`. Then A's stream is cut. The case asserts A gets `ModelError::Transport` and that the error does not name B's file. **Red now.**
- First run, the case alone (`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-tool-call --test adversary_w2026_11_09_u4 -- --exact another_logins_refusal_is_not_this_calls_failure`):
```
test another_logins_refusal_is_not_this_calls_failure ... FAILED
thread 'another_logins_refusal_is_not_this_calls_failure' (3958136) panicked at crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs:346:5:
a call on login A whose stream was cut came back as login B's refusal: Err(MissingCredential("no Codex access token in <worktree>/target/tmp/llm-tool-call-adversary-w2026-11-09-u4/cross-login-absent-3958135/auth.json; run `codex` to log in"))
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s
exit 101
```

**3. Suite run** (`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-tool-call --no-fail-fast`, after the case existed)
```
unittests src/lib.rs                       ok. 4 passed; 0 failed; 0 ignored
adversary_model.rs                         ok. 11 passed; 0 failed; 0 ignored
adversary_w11_call_tool_no_runtime.rs      ok. 1 passed; 0 failed; 0 ignored
adversary_w2026_11_09_u4.rs                FAILED. 3 passed; 1 failed; 0 ignored
  test another_logins_refusal_is_not_this_calls_failure ... FAILED
forced_tool_call.rs                        ok. 3 passed; 0 failed; 0 ignored
doc-tests                                  ok. 0 passed
error: 1 target failed:
exit 101
```
- 23 cases ran. The "before" count of 22 is this same run without my case; I did no separate run before writing the case.
- These four ran green in this run:
  - the acceptance case `a_refusal_on_a_spawned_task_is_still_a_credential_refusal`, now without `#[ignore]`
  - `opaque_state_crosses_two_models_of_one_login`
  - `a_concurrent_calls_refusal_is_not_another_calls_failure`
  - `two_concurrent_spawned_refusals_are_both_credential_refusals`

**4. Finding**

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| `crates/llm-tool-call/src/lib.rs:318` (with `:268`) | INFEASIBLE | introduced | `adversary_w2026_11_09_u4.rs:346` exit 101. Login A's cut stream comes back as `MissingCredential` naming login B's file. | The slot is keyed by `Provenance`, which leaves out the `auth.json` path, so different logins share one slot. Any turn outside `call_tool` (control-plane `loom_model.rs:323` calls `provider.turn` on `codex_model`) writes that slot. `call_tool` then reads it for any failed turn whose task-local slot is empty. That needs a direct turn and a concurrent `call_tool` on the same model id in one process; no in-repo process does both. |

- Cause: on any failure, `call_tool` falls back to the shared slot without checking that the turn's error could be a login refusal at all. At base `1a728949` only the task-local slot was read, so A would have got `Transport`. I reasoned this from the diff and did not run the case at base.
- Fix, named and not applied: consult the shared slot only when the turn's error is the untyped refusal the resolver produces (`Unauthorized`/`Unavailable`, request not sent). This classifies by error code, not message text.
- Doc gap: the `call_tool` doc names only the lost-refusal limit (`:290-294`), not this wrong-class result.

**5. Attacked, not broken**
- Two logins with equal provenance called one after the other: each `call_tool` clears the slot when it starts, so nothing crosses. Checked by reading, not by a case.
- A model that spawns only sometimes: a spawned refusal whose turn still succeeded stays in the slot only until the next call clears it. A stale read needs a spawned task that outlives its call; I built no case for that and found no caller that does it.
- Correction scope: the account id is `codex-subscription` again. In `b3bdb20a...068405b4`, case 3 is the only assertion that changed (the documented flip), plus a rustfmt reflow in case 2. `adversary_model.rs` only lost its `#[ignore]`.

**6. Paths written outside the worktree** (under the unit's assigned `scratch-adv2/` directory)
- `case.rs`
- `case-run.log`
- `suite-run.log`

The test fixtures went under the worktree's `target/tmp`. The lease `adv2-llm-u4` is released.

```findings
- file: crates/llm-tool-call/src/lib.rs
  line: 318
  category: concurrency
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "call_tool reads the provenance-keyed shared slot for any failed turn, so a refusal written by a concurrent turn outside call_tool on another login with equal provenance turns this call's Transport failure into that login's MissingCredential, naming the other file"
```
