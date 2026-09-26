# Wave 2 — proposal

Status: **approved by the operator 2026-09-26; running.** Skill `aep:implementing` 0.14.2; `aep` 0.60.0.
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
| integration | `wave/2` | `wave2-integration` | `~/.cache/b10x-target/llm-wave2-integration` | `~/.cache/llm-wave-2/integrate` | opening commit |
| fallback | `impl/ordered-fallback` | `wave2-fallback` | `~/.cache/b10x-target/llm-wave2-fallback` | `~/.cache/llm-wave-2/fallback` | dispatching |
| runpod | `impl/runpod-hosting` | `wave2-runpod` | `~/.cache/b10x-target/llm-wave2-runpod` | `~/.cache/llm-wave-2/runpod` | dispatching |
| toolname | `impl/streamed-tool-call-name` | `wave2-toolname` | `~/.cache/b10x-target/llm-wave2-toolname` | `~/.cache/llm-wave-2/toolname` | dispatching |

Worktree paths are `~/.local/state/worktree/trees/b10x/llm/<worktree id>`. Briefs are
`~/.cache/llm-wave-2/<unit>/brief.md`, invariants `~/.cache/llm-wave-2/invariants.md`.

Dispatch types: `aep:implementor`, then `aep:adversary` per unit. Builds run under `nice -n 19`,
`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=4`.

## The commits approval authorises

The opening store commit on `wave/2`; one commit per unit (3); three merges into `wave/2`; the
closing store commit; the merge of `wave/2` into `plan/llm-foundation`. No push, no tag, no
release, no merge into `main`.

## Later waves

| Wave | Stories | Held by |
|---|---|---|
| 3 | `story:unattributed-opaque-state` | wave 2 (selection.rs, projection observers) |
| 4 | `story:gateway-translation` | `credential-blocker:modal-live-deployment` via `story:modal-hosting` |
| 5 | `story:operator-cli` | wave 4 |
| 6 | `story:foundation-qualified` | the access decision blocker, wave 5 |
