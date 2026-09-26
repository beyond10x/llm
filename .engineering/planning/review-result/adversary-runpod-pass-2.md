---
format: aep.planning-md/1
id: review-result:adversary-runpod-pass-2
kind: review-result
status: active
title: 'Adversary, Runpod hosting, pass 2: takeover leaves an inherited pod billing, a not-ready pod outlives its deadline, request ids overflow'
relations:
- reviews: story:runpod-hosting
revision: 1
---
```
unit: story:runpod-hosting (impl/runpod-hosting, base d5c73e8), uncommitted working tree at /home/timo/.local/state/worktree/trees/b10x/llm/wave2-runpod after correction round 1
verdict: NEEDS-CHANGE
cases: executed 44→47, red 3
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 5 paths under /home/timo/.cache/llm-wave-2/runpod/adversary-2/ (part 6)
needs-coordinator: F7 may need a change in llm-provision (a takeover rule for resources whose owner label can't be rewritten), or a decision that takeover by another controller is out of scope for Runpod
```

**1. `git --no-pager diff --stat`**
```
 Cargo.lock                   |  3 +++
 crates/llm-runpod/Cargo.toml |  6 ++++++
 crates/llm-runpod/src/lib.rs | 44 +++++++++++++++++++++++++++++++++++++++++++-
 docs/hosting.md              | 44 ++++++++++++++++++++++++++++++++++++++++++++
 4 files changed, 96 insertions(+), 1 deletion(-)
```
All four paths belong to the implementor. The implementation is untracked, so it doesn't appear in `--stat`. I added one untracked file, `crates/llm-runpod/tests/adversary2.rs`. All seven `src/*.rs` files still match `src.sha256.c1` (`sha256sum -c`, 0 mismatches). I did not touch `tests/adversary.rs` or `tests/runpod.rs`.

**2. Cases added** (`crates/llm-runpod/tests/adversary2.rs`). Each was run alone with `--exact` before the suite, and each is red now.

| case | asserts |
|---|---|
| `a_pod_inherited_through_a_lease_takeover_is_served_or_stopped_by_its_new_owner` | after controller-a's lease expires and controller-b restores its snapshot, controller-b either serves pod-1 or terminates it |
| `a_pod_that_stops_serving_after_it_served_is_retired_within_its_startup_deadline` | a pod that served once and then answers NotReady for 660 s (deadline 600 s, idle timeout 1800 s) is terminated |
| `a_pool_that_accepts_its_controller_and_alias_can_start_a_pod` | a pool built over a 120-byte controller id and a 120-byte alias either refuses at construction or submits a create |

Red output, verbatim:
```
panicked at crates/llm-runpod/tests/adversary2.rs:142:5:
the inherited pod-1 is neither served nor stopped by the new owner: answers [Err("stopping"), Err("stopping"), Err("stopping"), Err("stopping"), Err("stopping")], record Some((StopRequired, Some(OwnershipLost))), terminations []

panicked at crates/llm-runpod/tests/adversary2.rs:224:5:
pod-1 has not served for 660 s against a 600 s deadline and is still billed; answers [Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting"), Err("starting")]

panicked at crates/llm-runpod/tests/adversary2.rs:255:5:
assertion `left == right` failed: a pool constructed over a 120-byte controller id and a 120-byte alias refuses every create (create calls: 0)
  left: Some(Hosting(InvalidSpec))
 right: Some(Starting)
```

**3. Suite, run after the cases existed.** Command: `nice -n 19 cargo test -p b10x-llm-runpod --locked --no-fail-fast`
```
unittests src/lib.rs   test result: ok. 1 passed; 0 failed
tests/adversary.rs     test result: ok. 6 passed; 0 failed
tests/adversary2.rs    test result: FAILED. 0 passed; 3 failed
tests/runpod.rs        test result: ok. 37 passed; 0 failed
EXIT=101
```
- `<before>` = 44 is the implementor's `cases:` line from `gate-c1-test.log`.
- `-- --list` counts 47 tests.
- Clippy with `-D warnings` and `cargo fmt -p b10x-llm-runpod --check` are both clean.

**4. Findings.** They cover the working tree above. Every origin is `introduced`: at the base, `crates/llm-runpod` was a scaffold.

| # | file:line | verdict | what was measured | what reaches it |
|---|---|---|---|---|
| F7 | pool.rs:579 | NEEDS-CHANGE | After a takeover, the record ends in `stop-required/ownership-lost`. `submit_owed_stops` never submits a stop for it. The new owner's sweep skips pod-1 because its tag names controller-a. `ensure` answers `stopping` every time and pod-1 is never terminated. The cause: the owner goes into the pod's env at create time (provider.rs:157), and the transport has no operation that can rewrite it. | The contract describes "lease expiry, reacquisition by another controller, Observe, Stop" as an ordinary sequence (`docs/hosting.md:142`, `:170`). `RunpodPool::restore` accepts any controller id. No caller in this repo does a takeover yet. The existing handover test only works because it uses the emulator's `retag`, which production code never does. |
| F8 | pool.rs:571 | INFEASIBLE | Once `ready_seen` is set, the startup deadline no longer applies. A pod that stops serving then answers `starting` until the idle reaper stops it, up to a full idle timeout (1800 s here). Nothing replaces it sooner. llmgw's `invalidate_endpoint` has no counterpart here. | A Running pod that keeps saying "not ready" had to be built with a wrapper transport (the emulator can't script it). I did not show that vLLM actually gets into that state. |
| F9 | pool.rs:700 | INFEASIBLE | The request id now joins the controller id and the alias, each valid up to 256 bytes. Together with the fixed parts they pass 256 bytes long before either part does. Every create is then refused as `hosting/invalid-spec`, and `RunpodPool::new` doesn't warn at construction. | Nothing found that uses ids of that length. |
| J5 | docs/hosting.md:304 | INFEASIBLE | The doc says the request id means a lost create is "never" resolved against another controller's pod. The `-` delimiter is ambiguous: controller `c-qwen` with alias `x` and controller `c` with alias `qwen-x` produce the same id when the millisecond and nonce also match. | That needs two processes creating in the same millisecond. It can't be built in one process, because the nonce counter is shared there. No case written. |
| J6 | provider.rs:179 | CONFIRMED | Since J1 removed the prune loop, `Health::restarts` only grows. `unserviceable` filters by the window but never removes old entries. | Any long-lived pod that restarts. It is only memory, and small. |

Suggested fixes, which I did not apply:
- **F7:** either make ownership read from something the new owner can change, or have the contract adopt a resource whose tag names the previous lease holder at an older epoch. The second option is in llm-provision.
- **F8:** keep a deadline running after a pod stops being ready.
- **F9:** check at construction that the composed request id fits, or build it from a fixed-size digest.

**5. Attacked and could not break**
- The new request id is unique within one process and across a restart without the snapshot.
- F2: request ids protect a pod only for records that don't have a key yet.
- F3: an EXITED pod is retired and terminated, including when it is an orphan.
- J1: the crash window's inclusive end is tested on both sides.
- J3: the endpoint is only reported for a pod in `Running` (checked through both `describe` and `ensure`).
- J4: in-flight counting follows the deployment through restore and replacement.
- The clock is always read after the lock, so `clock-reversed` can't happen.
- Deployment id generation.
- The two-live-records-per-alias theory from pass 1 still holds.

**6. Paths written outside the worktree** (all under `/home/timo/.cache/llm-wave-2/runpod/adversary-2/`)
- `build.log`, `suite.log`
- the three `alone-<case>.log` files, one per case in part 2

I made no copy of the worktree. Builds used the assigned `$HOME/.cache/b10x-target/llm-wave2-runpod`. The session lease `adversary-2-runpod-wave2` was acquired and released.

**7.**
```findings
- file: crates/llm-runpod/src/pool.rs
  line: 579
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: after the documented lease takeover by another controller, the inherited pod's owner tag cannot be rewritten, so the record parks in ownership-lost, no stop is ever submitted, the sweep skips it, and the alias answers stopping while the pod bills
- file: crates/llm-runpod/src/pool.rs
  line: 571
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: once a pod has served, ready_seen disables the startup deadline, so a pod that later stays not-ready is only removed by the idle reaper a full idle timeout later
- file: crates/llm-runpod/src/pool.rs
  line: 700
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the composed request id overflows the 256-byte identifier limit for a controller id and alias the pool accepts at construction, so every create fails as hosting/invalid-spec
- file: docs/hosting.md
  line: 304
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the dash-joined request id is ambiguous across controller/alias splits, so "never resolved against another controller's pod" holds only when two processes do not create in the same millisecond with the same nonce
- file: crates/llm-runpod/src/provider.rs
  line: 179
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: with the prune loop deleted, per-pod restart timestamps accumulate for the pod's whole life
```
