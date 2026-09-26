# Wave 2 — proposal

Status: **closed 2026-09-26: three stories implemented on one gate run.** Skill `aep:implementing` 0.14.2; `aep` 0.60.0.
Base: `plan/llm-foundation` at `c01143c` on origin.

`c01143c` merges the wave 1 close (`be36e1f`) with `f27b836`. Another session had reset the local
`plan/llm-foundation` to origin at `9fc2f5c`, committed `f27b836` on top and pushed it, which left the
46 wave 1 commits on no remote branch. The merge applied without conflicts; its gate ran every
`task check` step with exit 0, conformance 391/391 in three runs. Pushed through the bot as a
fast-forward `f27b836..c01143c`. The six `wave1-*` trees were then removed through `worktree gc`
and the six `impl/*` wave 1 branches deleted with `git branch -d`.

## Units

| Unit | Story | Serves | Scope |
|---|---|---|---|
| fallback | `story:ordered-fallback` | `vision:portable-model-inference` | 1 cited, 7 inferred |
| runpod | `story:runpod-hosting` | `vision:portable-model-inference` | 2 cited, 6 inferred |
| toolname | `story:streamed-tool-call-name` | `vision:portable-model-inference` | 11 cited, 5 inferred |

N = 3. Selection path: `aep plan artifact waves --kind story --status draft`, typed scope written
2026-09-26 from four `aep:story-scoper` runs.

## What the verb returned (verbatim)

```
wave 1
  story:anthropic-access (inferred)
  story:connectors-secret-resolver (inferred)
  story:modal-hosting (inferred)
  story:streamed-tool-call-name (inferred)
wave 2
  story:openai-access (inferred)
  story:ordered-fallback (inferred)
wave 3
  story:runpod-hosting (inferred)
wave 4
  story:unattributed-opaque-state (inferred)
wave 5
  story:gateway-translation (inferred)
wave 6
  story:operator-cli (inferred)
wave 7
  story:foundation-qualified (inferred)
collision: story:anthropic-access story:openai-access crates/llm-providers (inferred)
collision: story:connectors-secret-resolver story:ordered-fallback spec/domains/catalog.yaml (inferred)
collision: story:connectors-secret-resolver story:runpod-hosting spec/domains/catalog.yaml (inferred)
collision: story:connectors-secret-resolver story:unattributed-opaque-state spec/domains/catalog.yaml (inferred)
collision: story:ordered-fallback story:runpod-hosting spec/domains/catalog.yaml (inferred)
collision: story:ordered-fallback story:unattributed-opaque-state contracts/routing/scenarios (inferred)
collision: story:ordered-fallback story:unattributed-opaque-state crates/llm-routing/src/selection.rs
collision: story:ordered-fallback story:unattributed-opaque-state spec/domains/catalog.yaml (inferred)
collision: story:ordered-fallback story:unattributed-opaque-state spec/domains/routing.yaml (inferred)
collision: story:runpod-hosting story:unattributed-opaque-state spec/domains/catalog.yaml (inferred)
collision: story:streamed-tool-call-name story:unattributed-opaque-state checks/conformance/src/chat.rs
collision: story:streamed-tool-call-name story:unattributed-opaque-state checks/conformance/src/messages.rs
collision: story:streamed-tool-call-name story:unattributed-opaque-state checks/conformance/src/responses.rs
collision: story:streamed-tool-call-name story:unattributed-opaque-state contracts/chat/scenarios
collision: story:streamed-tool-call-name story:unattributed-opaque-state contracts/messages/scenarios
collision: story:streamed-tool-call-name story:unattributed-opaque-state contracts/responses/scenarios
collision: story:streamed-tool-call-name story:unattributed-opaque-state crates/llm-chat/src/ingress.rs
collision: story:streamed-tool-call-name story:unattributed-opaque-state crates/llm-responses/src/stream.rs (inferred)
collision: story:streamed-tool-call-name story:unattributed-opaque-state spec/domains/chat.yaml (inferred)
collision: story:streamed-tool-call-name story:unattributed-opaque-state spec/domains/responses.yaml (inferred)
7 wave(s), 20 collision(s), 0 unassessed
```

## Where this proposal departs from the verb

The verb's layers include stories that are not ready. Removing them:

| Story | Why it is out |
|---|---|
| `story:anthropic-access`, `story:openai-access` | open `decision-blocker:subscription-access-contract` |
| `story:connectors-secret-resolver` | open `dependency-blocker:connectors-arbitrary-secrets` |
| `story:modal-hosting` | acceptance needs a live paid deployment; `credential-blocker:modal-live-deployment` filed 2026-09-26 |
| `story:runtime-contracts` | active; remaining work is workspace manifests and release setup, which every unit touches |

What is left, in the verb's order: toolname (layer 1), fallback (layer 2), runpod (layer 3),
opaque (layer 4). The verb places fallback and runpod in separate layers because of **one**
inferred collision each on `spec/domains/catalog.yaml`. Toolname collides with neither.

**Coordinator decision:** in wave 2, `spec/domains/catalog.yaml` belongs to the coordinator. The
fallback unit reuses the existing `Rejection` variants; the runpod unit keeps GPU, cache, deadline
and vLLM settings in an adapter-local config type rather than `DeploymentSpec`. With that, the
three units have no shared surface. If either unit finds it cannot finish without editing
`catalog.yaml`, it hands the edit back, and the coordinator writes it.

`story:unattributed-opaque-state` collides with fallback on `crates/llm-routing/src/selection.rs`
(cited) and with toolname on 10 surfaces. It is wave 3, alone.

## Decisions taken for the briefs

| Unit | Decision | Source |
|---|---|---|
| toolname | No envelope version moves. `StreamEvent` is in neither `llm.turn/2` (`crates/llm-core/src/turn.rs:395`) nor `llm.outcome/3` (`:417`); bumping either would collide with wave 3 on 74 scenario files | scoper report |
| toolname | The four exhaustive `StreamEvent` matches are edited: `crates/llm-chat/src/ingress.rs:430`, `checks/conformance/src/{chat.rs:307,messages.rs:228,responses.rs:276}` | scoper report |
| fallback | Evidence is ESS scenarios under `contracts/routing/scenarios` and `spec/domains/routing.yaml`, as `story:catalog-routing` did | scoper report |
| fallback | "Exhausted limits" is a caller-supplied limit decision; no `b10x-llm-cost` dependency | coordinator |
| runpod | Pod name prefix differs from llmgw's `llmgw-` (`llmgw/src/runpod.rs:21`); the new orphan sweep never touches `llmgw-` pods | scoper report |
| runpod | Emulation through a transport trait with a fake; no new HTTP server dependency | coordinator |
| runpod | Port from `llmgw/src/runpod.rs` (992 lines) and its mock tests in `llmgw/src/lib.rs`, with attribution | scoper report |

## Coordinator-owned

Same as wave 1: workspace `Cargo.toml`, `Cargo.lock`, `contracts/ess-inputs.yaml`,
`contracts/suite.json`, `contracts/baseline.json`, `contracts/schema`,
`checks/conformance/src/target.rs` and `main.rs`, `docs/implementation-status.md`, `Taskfile.yml`,
everything under `.engineering`, and for this wave `spec/domains/catalog.yaml`.

## Pre-flight (2026-09-26)

| Check | Value | Refuses? |
|---|---|---|
| free disk on `/` | 34G after the integration gate's build directory was deleted (`df -h /`) | no; floor 10G |
| wave 1 unit trees | removed (`worktree gc --apply`, six ids) | no |
| other trees in this repository | `llm-plan-llm` (`wave/1`), `wt-7a74eb8a79f2` (`plan/llm-foundation`, another session), 2 detached `wt-*` at `9fc2f5c` | no; not this wave's, left in place |
| other build dirs | `~/.cache/b10x-target/llm-pr1-partB` 2.6G, `llm-review-a51a5e` 684M | no; ownership unknown, left in place |
| one measured build | integration gate, full workspace: 2.7G | sizes N=3 at about 8G |

## Units — planned records

| Unit | Branch | Worktree id | Build dir | Scratch root | Stage |
|---|---|---|---|---|---|
| integration | `wave/2` | `wave2-integration` | `~/.cache/b10x-target/llm-wave2-integration` | `~/.cache/llm-wave-2/integrate` | closed |
| fallback | `impl/ordered-fallback` | `wave2-fallback` | `~/.cache/b10x-target/llm-wave2-fallback` | `~/.cache/llm-wave-2/fallback` | merged `af5730a` |
| runpod | `impl/runpod-hosting` | `wave2-runpod` | `~/.cache/b10x-target/llm-wave2-runpod` | `~/.cache/llm-wave-2/runpod` | merged `2359133` |
| toolname | `impl/streamed-tool-call-name` | `wave2-toolname` | `~/.cache/b10x-target/llm-wave2-toolname` | `~/.cache/llm-wave-2/toolname` | merged `5b789df` |

Worktree paths are `~/.local/state/worktree/trees/b10x/llm/<worktree id>`. Briefs are
`~/.cache/llm-wave-2/<unit>/brief.md`, invariants `~/.cache/llm-wave-2/invariants.md`.

Dispatch types: `aep:implementor`, then `aep:adversary` per unit. Builds run under `nice -n 19`,
`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=4`.

## The commits approval authorises

The opening store commit on `wave/2`; one commit per unit (3); three merges into `wave/2`; the
closing store commit; the merge of `wave/2` into `plan/llm-foundation`. No push, no tag, no
release, no merge into `main`.

Deviation: `827d574` regenerates the coordinator-owned contract files. The list above did not
name it; wave 1 made the same commit (`14e8691`) for the same reason, and the gate cannot pass
without it.

## The close

| Unit | Adversary passes | Findings per pass | Corrections | Merged |
|---|---|---|---|---|
| fallback | 2 | 4, then 4 (0 carried) | 1 | `af5730a` |
| toolname | 2 | 4, then 6 (0 carried) | 2; the second reviewed by the coordinator | `5b789df` |
| runpod | 2 | 10, then 5 (0 carried) | 2; the second reviewed by the coordinator | `2359133` |

Gate on `wave/2` at `827d574`, one exit status per step: `cargo test --workspace`, the credentials
and cost feature lanes, `fmt`, `clippy`, `ess specify validate`, `conformance -- check` and
`aep plan artifact validate` all exit 0. Conformance 410/410 in three runs. One `test_result`
recorded against `827d574` moved all three stories to `implemented`.

### Findings nobody acted on

| Story | Finding | Why it is open |
|---|---|---|
| `story:ordered-fallback` | `review-result:adversary-fallback-pass-2` D: foreign evidence on a refused `Ok` outcome is recorded `Dispatch::Accepted`, on an `Err` path `Dispatch::Unknown` | only a third-party `Model` reaches it; both paths halt |
| `story:runpod-hosting` | `review-result:adversary-runpod-pass-1` J2: orphan and inherited pod terminations never reach the budget ledger as stop obligations | needs a change in `crates/llm-provision`; documented in `docs/hosting.md` |

### What it cost

From the harness's completion reports. A resumed agent's figure is its report for that round.

| Agent | Tokens | Tool uses | Wall |
|---|---|---|---|
| scopers (4) | 223,830 | 58 | 8m05s |
| implementor fallback, rounds 0–1 | 164,606 + 189,493 | 88 + 31 | 13m41s + 4m20s |
| implementor toolname, rounds 0–2 | 246,941 + 249,429 + 276,887 | 171 + 21 + 23 | 18m38s + 3m36s + 3m39s |
| implementor runpod, rounds 0–2 | 225,434 + 282,722 + 333,216 | 62 + 45 + 26 | 26m18s + 7m00s + 6m23s |
| adversary fallback, passes 1–2 | 101,906 + 117,238 | 39 + 39 | 5m30s + 6m22s |
| adversary toolname, passes 1–2 | 88,684 + 150,832 | 26 + 58 | 4m55s + 9m28s |
| adversary runpod, passes 1–2 | 171,180 + 140,864 | 36 + 37 | 8m30s + 9m14s |

## Later waves

| Wave | Stories | Held by |
|---|---|---|
| 3 | `story:unattributed-opaque-state` | nothing; wave 2 is merged |
| 4 | `story:gateway-translation` | `credential-blocker:modal-live-deployment` via `story:modal-hosting` |
| 5 | `story:operator-cli` | wave 4 |
| 6 | `story:foundation-qualified` | the access decision blocker, wave 5 |
