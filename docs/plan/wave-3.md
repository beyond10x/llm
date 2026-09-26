# Wave 3

Status: **running; approved by the operator 2026-09-26** ("then keep going with the next wave").
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
| The unit owns the 74 scenario files that embed `llm.turn/2` and `docs/contract-v1.md` | coordinator |

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
| integration | `wave/3` | `wave3-integration` | `~/.cache/b10x-target/llm-wave3-integration` | `~/.cache/llm-wave-3/integrate` | opening commit |
| opaque | `impl/unattributed-opaque-state` | `wave3-opaque` | `~/.cache/b10x-target/llm-wave3-opaque` | `~/.cache/llm-wave-3/opaque` | dispatching |

## The commits this wave makes

The opening store commit on `wave/3`; one unit commit; one merge into `wave/3`; the contract
regeneration commit; the closing store commit; the merge of `wave/3` into `plan/llm-foundation`
and its push through the bot. No tag, no release, no merge into `main`.
