# Wave 3

Status: **closed 2026-09-26: one story implemented.** Approved by the operator ("then keep going with the next wave").
Skill `aep:implementing` 0.14.2; `aep` 0.60.0. Base: `plan/llm-foundation` at `87070c3` on origin.

## Unit

| Unit | Story | Serves | Scope |
|---|---|---|---|
| opaque | `story:unattributed-opaque-state` | `vision:portable-model-inference` | 16 cited, 5 inferred (written 2026-09-26 by `aep:story-scoper`) |

N = 1. It collided with `story:ordered-fallback` on `crates/llm-routing/src/selection.rs` and with
`story:streamed-tool-call-name` on 10 surfaces (`docs/plan/wave-2.md`); both are merged. Every other
remaining story is blocked or depends on an unfinished one:

| Story | Held by |
|---|---|
| `story:anthropic-access`, `story:openai-access` | `decision-blocker:subscription-access-contract` |
| `story:connectors-secret-resolver` | `dependency-blocker:connectors-arbitrary-secrets` |
| `story:modal-hosting` | `credential-blocker:modal-live-deployment` |
| `story:gateway-translation` | `story:modal-hosting`, `story:unattributed-opaque-state` |
| `story:operator-cli`, `story:foundation-qualified` | later stories |

## Decisions taken for the brief

| Decision | Source |
|---|---|
| Both envelopes move: `llm.turn/2` → `llm.turn/3` and `llm.outcome/3` → `llm.outcome/4`, because both carry `Item` | coordinator; `crates/llm-core/src/turn.rs:395`, `:417` |
| Routing reuses `Rejection::OpaqueState` for the new state; no `spec/domains/catalog.yaml` change | coordinator; scoper report |
| "Gateway round trip" is a projection ingress followed by the same projection's egress; the gateway translation itself is `story:gateway-translation` | coordinator; `crates/llm-gateway` has no `Item` use |
| The unit owns the 85 scenario files that embed `llm.turn/2` and `docs/contract-v1.md` | coordinator |

## Coordinator-owned

As in wave 2: workspace `Cargo.toml`, `Cargo.lock`, `contracts/ess-inputs.yaml`,
`contracts/suite.json`, `contracts/baseline.json`, `contracts/schema`,
`checks/conformance/src/target.rs` and `main.rs`, `docs/implementation-status.md`, `Taskfile.yml`,
`spec/domains/catalog.yaml`, everything under `.engineering`.

## Pre-flight (2026-09-26)

| Check | Value |
|---|---|
| free disk on `/` | 15G (`df -h /`); floor 10G |
| measured builds | unit 3.3G (wave 2 toolname), full gate 2.7G; the unit build dir is deleted before the integration gate |
| wave 2 trees | removed (`worktree gc --apply`) |

## Unit record

| Unit | Branch | Worktree id | Build dir | Scratch root | Stage |
|---|---|---|---|---|---|
| integration | `wave/3` | `wave3-integration` | `~/.cache/b10x-target/llm-wave3-integration` | `~/.cache/llm-wave-3/integrate` | closed |
| opaque | `impl/unattributed-opaque-state` | `wave3-opaque` | `~/.cache/b10x-target/llm-wave3-opaque` | `~/.cache/llm-wave-3/opaque` | merged `ee4b7a7` |

## The commits this wave makes

The opening store commit on `wave/3`; one unit commit; one merge into `wave/3`; the contract
regeneration commit; the closing store commit; the merge of `wave/3` into `plan/llm-foundation`
and its push through the bot. No tag, no release, no merge into `main`.

## The close

| Adversary passes | Findings per pass | Corrections | Merged |
|---|---|---|---|
| 2 | 5, then 2 (0 carried, per `aep plan artifact findings`) | 2; the second reviewed by the coordinator | `ee4b7a7` |

The first whole-repository gate on `wave/3` exited 1 at the conformance step, 422/423:
`contracts/inference/scenarios/outcome-version-4-refused` asserted that `llm.outcome/4` is a
refused future version, and this wave made it current. The unit's gate ran the chat, messages,
responses and routing lanes only. `952de3f` moves that scenario to `llm.outcome/5`, adds
`outcome-version-3-refused`, and regenerates the contract files. The eight Rust and ESS steps before
conformance exited 0 on the merged tree; after `952de3f`, which changes scenario files only,
`ess specify validate`, `conformance -- check` (424/424 in three runs) and
`aep plan artifact validate` were re-run and exit 0.

Coordinator decisions beyond the brief: the contract is JSON-equal, not byte-equal; Responses
ingress carries only `CARRIED_ENTRY_TYPES` (`reasoning`); route selection refuses unattributed
state as `opaque-state`, the same rejection as foreign state. `review-result:adversary-opaque-pass-2`
finding 2 (a gateway binding carried state to the reader reinstates laundering) is written into
`story:gateway-translation` under "Opaque state carried from ingress".

### What it cost

| Agent | Tokens | Tool uses | Wall |
|---|---|---|---|
| implementor, rounds 0–2 | 278,382 + 313,380 + 328,137 | 173 + 35 + 11 | 19m14s + 4m09s + 1m43s |
| adversary, passes 1–2 | 151,883 + 154,330 | 46 + 40 | 8m48s + 8m37s |
