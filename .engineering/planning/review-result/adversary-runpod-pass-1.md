---
format: aep.planning-md/2
id: review-result:adversary-runpod-pass-1
kind: review-result
status: active
title: 'Adversary, Runpod hosting, pass 1: three red cases on request identity, sweep protection and EXITED pods'
relations:
- reviews: story:runpod-hosting
revision: 1
---
```
unit: story:runpod-hosting (impl/runpod-hosting, base d5c73e8), uncommitted working tree at home-path:sha256:47b2652f354c9f6005baff67b7df8fc51fc511b454cf4e2b61d9482f3990d8b6
verdict: NEEDS-CHANGE
cases: executed 31→37, red 3
origin: introduced 12 / pre-existing 0 / undecided 0
wrote-outside-worktree: 11 paths under home-path:sha256:93efe81e599943c5502277134fbed32c663719887e383316be10cb8e6bb5208d (part 6)
needs-coordinator: none
```

**1. `git --no-pager diff --stat`**
```
 Cargo.lock                   |  3 +++
 crates/llm-runpod/Cargo.toml |  6 ++++++
 crates/llm-runpod/src/lib.rs | 44 +++++++++++++++++++++++++++++++++++++++++++-
 docs/hosting.md              | 35 +++++++++++++++++++++++++++++++++++
 4 files changed, 87 insertions(+), 1 deletion(-)
```
All four paths are the implementor's, unchanged from what I was handed. The implementation is untracked, so `--stat` can't show my file. My only change is the untracked `crates/llm-runpod/tests/adversary.rs`. All seven `src/*.rs` files still match `src.sha256.before1` (`sha256sum -c`: OK).

**2. Cases added** (`home-path:sha256:16b08056d1e2804182af2b0b421c061f072eb973b6ca99aa0c68c34a2d234431`). Each was run alone first, with `--exact`.

| case | asserts | now |
|---|---|---|
| `a_lost_create_is_not_resolved_against_another_controllers_pod_with_the_same_alias` | a lost create settles on the pod it created, even when controller-b has a pod tagged `request-qwen-1` | red |
| `a_leftover_pod_of_ours_is_swept_even_when_a_fresh_pool_reuses_its_request_id` | a pod of ours that no record holds under its exact key is swept | red |
| `an_exited_pod_is_terminated_rather_than_recorded_as_stopped` | a pod Runpod reports as EXITED is not recorded `Stopped` while it still exists | red |
| `the_crash_restart_limit_is_reached_at_exactly_its_count` | exactly 2 restarts with limit 2 gives `crash-loop` | green, catches mutant |
| `every_refused_setting_is_refused_on_its_own` | 11 validation clauses, one at a time, plus 1.0 accepted | green, catches mutants |
| `an_llmgw_pod_does_not_keep_a_lost_create_unresolved` | an untagged `llmgw-` pod does not block a lost create from being resolved | green, catches mutant |

Red output, verbatim:
```
panicked at crates/llm-runpod/tests/adversary.rs:131:5:
assertion `left == right` failed: the lost create must resolve to the pod it created, not to another controller's pod that happens to carry the same deterministic request id
  left: Err("stopping")
 right: Ok(Some("https://pod-2-8000.proxy.runpod.net"))

panicked at crates/llm-runpod/tests/adversary.rs:160:5:
assertion `left == right` failed: a pod tagged as ours that no record holds under its exact key is an orphan
  left: []
 right: [Identifier("pod-1")]

panicked at crates/llm-runpod/tests/adversary.rs:230:5:
qwen-1 is Stopped with evidence Some(Identifier("provider-terminated")), yet pod-1 still exists and was never terminated (terminations: [])
```

Mutation probes ran on a scratch copy, never on the tree. Each mutant was applied, both test binaries were run, and the file was restored (sha checked). With every mutant, `tests/runpod.rs` stayed green (30 passed, exit 0) and the named adversary case failed on an assertion (exit 101):

| mutant | location |
|---|---|
| `restarts >= limit` → `>` (fails: `Err("still-starting")` instead of `Err("crash-loop")`) | provider.rs:80 |
| `utilization <= 0.0` → `< 0.0` | config.rs:159 |
| empty secret accepted | config.rs:171 |
| `max_num_seqs == 0` clause dropped | config.rs:163 |
| inventory namespace filter → `\|_\| true` | provider.rs:238 |

**3. Suite, run after the cases existed.** `nice -n 19 cargo test -p b10x-llm-runpod --locked --no-fail-fast`:
```
unittests src/lib.rs   test result: ok. 1 passed; 0 failed
tests/adversary.rs     test result: FAILED. 3 passed; 3 failed
tests/runpod.rs        test result: ok. 30 passed; 0 failed
EXIT=101
```
Clippy (`-D warnings`) and `cargo fmt --check` are clean with the new file. `<before>` = 31 is the implementor's reported `cases:` line.

**Findings.** All cover the tree above; every origin is `introduced`, because the base crate was a scaffold (`git show d5c73e8:crates/llm-runpod/src/lib.rs`: "exports no runtime API yet").

| # | file:line | verdict | what was measured | what reaches it |
|---|---|---|---|---|
| F1 | pool.rs:683 | NEEDS-CHANGE | request id is `request-{alias}-{generation}`, the same for every controller. A lost create gets resolved against controller-b's pod. Its record goes to `stop-required/ownership-lost`, which `submit_owed_stops` never submits. `ensure` then answers `stopping` for good, and our own pod-2 is kept out of the sweep by the same request id. | two controllers in one account serving the same alias (the owner tag and the controller-b cases exist for exactly this), plus a lost create (`lose_next_create`, tested). Fix: put the controller id and a nonce in the request id. |
| F2 | pool.rs:642 | NEEDS-CHANGE | the sweep counts a pod as recorded when its request id matches a live record, even though that record already holds a different exact key. The leftover pod stays billed for as long as the new record lives. | a pool opened with `new` after a restart. Nothing in the crate stores the snapshot. The request-id branch is only needed for records with no key yet, and it runs after `Observe` has already adopted those by request id. Fix: protect only records whose `observed` is `None`. |
| F3 | provider.rs:98 | CONFIRMED | EXITED is mapped to Terminated. The record closes with `provider-terminated` evidence, and the sweep skips the pod forever. | Runpod's EXITED status (a stopped pod); the adapter declares the variant itself. The emulator can't produce it; my test built it with a wrapper transport. I did not measure the billing effect. Fix: treat EXITED as Running (still to terminate), not Terminated. |
| F4 | provider.rs:80 | CONFIRMED | the mutant `>=`→`>` survives the suite | a case that proves the boundary |
| F5 | config.rs:159 | CONFIRMED | utilization 0.0, an empty secret and a zero `max_num_seqs` are each accepted by a mutant the suite misses (lines 159/163/171) | three validation clauses nothing checks one at a time |
| F6 | provider.rs:238 | CONFIRMED | the mutant that drops the namespace filter survives. With it, an untagged llmgw pod would block every lost create for good. | llmgw pods in the same account |

**4. Judgement findings** (text only; none written to the store)
- provider.rs:137: `restarts` never resets. Because crash-loop counting applies "at any time", two container restarts spread over a 24 h life kill a healthy pod. Note.
- pool.rs:656: the orphan sweep calls `provider.stop` directly. There is no record and no stop obligation, so the budget ledger never sees the termination. Note.
- provider.rs:112: the endpoint is built from the pod id for every pod, including Pending and Exited ones. The contract says an endpoint the provider did not report is `None`. Note.
- pool.rs:192: `Usage` is per alias, not per deployment. A stream lease still open on a retired pod keeps its replacement from being idle-reaped. Note.

**5. Attacked and could not break**
- Single flight: ten concurrent callers, and a lock held across the whole step.
- The startup deadline boundary.
- The idle-limit floor, and stream-lease drop timing.
- Partial listings.
- The legacy-prefix constants.
- Unknown readiness.
- Generation-id collisions across aliases: the numeric suffix after the last `-` keeps them unique.
- A second live record per alias: `start` only runs when the slot is empty.

**6. Paths written outside the worktree** (all under `home-path:sha256:93efe81e599943c5502277134fbed32c663719887e383316be10cb8e6bb5208d`)
- `mutate.py`, `mutants.json`, `suite.log`
- six `alone-<case>.log` files
- five `mutant-<name>.log` files
- `copy/`, a 9.8M copy of the worktree
- `target/`, 96M, already deleted

Session lease `adversary-1-runpod-wave2` was acquired and released.

```findings
- file: crates/llm-runpod/src/pool.rs
  line: 683
  category: contract-drift
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the request id is deterministic across controllers, so a lost create is resolved against another controller's pod and the record parks in stop-required/ownership-lost for good while our own pod is never stopped
- file: crates/llm-runpod/src/pool.rs
  line: 642
  category: boundary
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the orphan sweep counts a matching request id as recorded even when that record already owns a different exact key, so a leftover pod of ours survives a restart that did not persist its snapshot
- file: crates/llm-runpod/src/provider.rs
  line: 98
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: EXITED is mapped to Terminated, so the record closes with provider-terminated evidence while the pod still exists and the sweep skips it forever
- file: crates/llm-runpod/src/provider.rs
  line: 80
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: weakening the crash-restart limit from >= to > leaves the existing suite green
- file: crates/llm-runpod/src/config.rs
  line: 159
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: mutants accepting zero GPU utilization, an empty secret name or zero max_num_seqs each survive the existing suite
- file: crates/llm-runpod/src/provider.rs
  line: 238
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: dropping the inventory namespace filter survives the suite although an untagged llmgw pod would then block every lost create for good
- file: crates/llm-runpod/src/provider.rs
  line: 137
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: container restarts accumulate over the pod's whole life and never reset, so sparse restarts terminate a healthy pod
- file: crates/llm-runpod/src/pool.rs
  line: 656
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: orphan terminations bypass the controller and never reach the budget ledger as a stop obligation
- file: crates/llm-runpod/src/provider.rs
  line: 112
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: an endpoint is fabricated for every pod including pending and exited ones, where the contract says an unreported endpoint is None
- file: crates/llm-runpod/src/pool.rs
  line: 192
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: in-flight accounting is per alias, so a stream lease on a retired pod keeps its replacement from being idle-reaped
```
