---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u4-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:call-tool-spawned-refusal-keeps-its-class, wave 2026-11-09-github-zero)
relations:
- reviews: story:call-tool-spawned-refusal-keeps-its-class
revision: 1
---
unit: llm u4 story:call-tool-spawned-refusal-keeps-its-class, commit fd54df39 plus one untracked test file in the unit tree llm-gh-zero-u4
verdict: red
cases: executed 19→22, red 3
origin: introduced 2 / pre-existing 1 / undecided 0
wrote-outside-worktree: 6 paths, all under the assigned scratch-adv directory (part 6)
needs-coordinator: finding 3 reproduces at base, so its origin is pre-existing, but it is inside this story's Outcome ("whichever task"). Decide whether it stays with this unit or goes to the backlog.

**1. `git --no-pager diff --stat`**

`git --no-pager diff --stat` prints nothing because I changed no tracked file. `git status --short` shows one new file:
```
?? crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs
```
That is a test file, so the scope bound held.

**2. Cases added** (in `crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs`). All three are red now. This is the first run of the file on its own (`cargo test -p llm-tool-call --test adversary_w2026_11_09_u4`, exit 101):
```
---- opaque_state_crosses_two_models_of_one_login stdout ----
panicked at crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs:162:5:
the state of one binding came back refused as another binding's: Model(Error { code: Unsupported, message: "opaque state belongs to a different serving binding", dispatch: NotSent, ... })
---- a_concurrent_calls_refusal_is_not_another_calls_failure stdout ----
panicked at crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs:200:5:
a call whose login was read and whose stream was cut came back as Err(MissingCredential("no Codex access token in .../concurrent-cross-2535204/auth.json; run `codex` to log in"))
---- two_concurrent_spawned_refusals_are_both_credential_refusals stdout ----
panicked at crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs:287:5:
the first call's refusal was cleared by the second call's start: Err(Model(Error { code: Unauthorized, message: "secret reference `codex-login` refused: secret reference was not found", ... }))
test result: FAILED. 0 passed; 3 failed
```
- `opaque_state_crosses_two_models_of_one_login`: two `codex_model_at` calls with the same model, URL and login. An opaque item carrying the first model's provenance goes back to the second, and the call fails as `MissingCredential`.
- `a_concurrent_calls_refusal_is_not_another_calls_failure`: one plain (non-spawning) model and two concurrent `call_tool` calls. Call A reads its login and its stream is cut. The login is then removed and call B is refused. A should be `Transport`, B `MissingCredential`.
- `two_concurrent_spawned_refusals_are_both_credential_refusals`: one spawning model, two concurrent calls, both refused for the same missing login. Both should be `MissingCredential`.

**Origin check against base.** I extracted `git archive 1a728949` into scratch, copied the same test file in and ran it there. No checkout or stash in the tree. Result: cases 1 and 2 `ok`, case 3 `FAILED` with the same `Model(Unauthorized …)` error. So 1 and 2 were introduced by this unit, and 3 is pre-existing.

**3. Suite run** (after the cases existed): `RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-tool-call --no-fail-fast`, exit 101
```
unittests src/lib.rs                      ok. 4 passed
tests/adversary_model.rs                  ok. 11 passed
tests/adversary_w11_call_tool_no_runtime  ok. 1 passed
tests/adversary_w2026_11_09_u4.rs         FAILED. 0 passed; 3 failed
tests/forced_tool_call.rs                 ok. 3 passed
Doc-tests llm_tool_call                   ok. 0 passed
```
- My first suite run without `--no-fail-fast` stopped at my binary, so I reran it.
- The `19` in "executed 19→22" is this same run with my file left out (4+11+1+3).
- The un-ignored `a_refusal_on_a_spawned_task_is_still_a_credential_refusal` passes.

**4. Findings** (cover fd54df39 plus the test file)

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| crates/llm-tool-call/src/lib.rs:250 | CONFIRMED | introduced | Case 1, :162: two models of one login no longer have the same `Provenance`, because the account id is now `codex-subscription-N`. `validate_for` therefore refuses opaque state from one in the other. The case passes at base. | control-plane `CodexAgentModel::respond_in` (`control-plane-runtime/src/loom_model.rs:52`, wired at `control-plane-app/src/cli.rs:340`) builds `codex_model` on every request. `ProviderPort::new` (:227) hashes `provenance()` into the continuation wire and refuses a foreign wire or binding (:239-253). After the upgrade, every resumed session's opaque continuation is refused. control-plane pins llm tag 0.4.0 today, so this hits it when it upgrades. |
| crates/llm-tool-call/src/lib.rs:277 | CONFIRMED | introduced | Case 2, :200: the refusal is written to the shared slot even on the task that awaits `call_tool`. A sibling call whose task-local slot is empty then takes it (:324-327), so a cut stream comes back as `MissingCredential`. The case passes at base. The doc at :233-234 ("on the awaiting task the task-local slot wins") hides this. | Any two concurrent `call_tool` calls on one Codex model when the login goes bad (expires) mid-call. The API allows it (`&dyn Model`; loom-plugin `classify`, loom-intake-router `classify_with_catalog`). I found no caller that shares one model across concurrent calls. |
| crates/llm-tool-call/src/lib.rs:313 | CONFIRMED | pre-existing | Case 3, :287: call B's start clears the slot (:314-315) after A's spawned refusal is written, so A comes back as `ModelError::Model`. The story's Outcome, "whichever task", does not hold under concurrency. | Two concurrent calls on one spawning model. No caller found. |

Fixes I would suggest but did not apply:
- **Finding 1:** keep the account id `codex-subscription`, and key `REFUSAL_SLOTS` by the whole `Provenance`. Models with identical provenance would then share one slot.
- **Finding 2:** write the shared slot only when `LOGIN_REFUSAL.try_with` fails, i.e. off the awaiting task.
- **Finding 3:** needs a per-call token that crosses the spawn, or a documented limit.

**5. Attacked and could not break**
- No catalog, routes document or llm crate names `codex-subscription`. The only other hits are intake's own copy and a control-plane run log.
- A stale refusal left by an earlier turn or a direct `turn()` is cleared when the next `call_tool` starts.
- The `Weak` map: lookup and registration share one mutex, dead entries are pruned on each registration (so at most a lag of one), and the `u64` counter cannot wrap in practice.
- The fallback case (`a_kept_refusal_does_not_override_a_turn_that_succeeded`) still passes.

**6. Paths written outside the worktree** (all under the unit's assigned `scratch-adv/` directory)
- `case-run.log`
- `suite-run.log`
- `suite-run-nff.log`
- `base-run.log`
- `base/`: git archive of 1a728949 plus a copy of the test file
- `base-target/`: cargo target directory for the base build

Fixture `auth.json` files go under the worktree's own `target/tmp/llm-tool-call-adversary-w2026-11-09-u4/`. I acquired and released the lease (`adv-llm-u4`).

**7.**
```findings
[
  {"file": "crates/llm-tool-call/src/lib.rs", "line": 250, "category": "contract-drift", "severity": "blocker", "verdict": "CONFIRMED", "origin": "introduced", "message": "a per-model account id gives two Codex models of one login different provenance, so opaque continuation state is refused across them and control-plane's per-request codex_model breaks session resume"},
  {"file": "crates/llm-tool-call/src/lib.rs", "line": 277, "category": "concurrency", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "the refusal is also written to the shared slot on the awaiting task, so a concurrent sibling call on a plain model takes it and reports a transport failure as MissingCredential"},
  {"file": "crates/llm-tool-call/src/lib.rs", "line": 313, "category": "concurrency", "severity": "warning", "verdict": "CONFIRMED", "origin": "pre-existing", "message": "a second call_tool on a spawning model clears the shared slot after the first call's spawned refusal, so the first comes back as ModelError::Model"}
]
```
