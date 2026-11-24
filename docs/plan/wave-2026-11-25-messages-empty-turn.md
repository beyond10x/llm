# Wave 2026-11-25-messages-empty-turn

Status: **open**. Wave 4 of the [wave plan](trajectory.md), the first of the four stories accepted on
2026-11-24. Approval: `approval-record:wave-2026-11-25-messages-empty-turn` (the owner's order of
2026-11-24 to run the next wave from the active stories, relayed by the operator session). Opened on
the release commit of 0.29.0, `bec3606f9`. Integration branch `wave/2026-11-25-messages-empty-turn`,
tree `llm-1125-empty-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: issue 83 of this repository, a Messages turn that stops for an unmodelled reason
without content and decodes as a success.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, after the four
accepted stories got their Scope sections (one `story-scoper` each, read at `87bc16f16`):

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:classifier-takes-only-a-whole-answer (inferred)
wave 3
  story:ess-pin-current-and-local-gate-finds-it (inferred)
wave 4
  story:messages-empty-turn-with-an-unmodelled-stop-reason (inferred)
4 wave(s), 38 collision(s), 0 unassessed
```

N is one. `story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification` and
is not in the plan. Every pair of the four stories collides on `AGENTS.md` and `CHANGELOG.md`; the
two that add scenarios also collide on `contracts/ess-inputs.yaml`, `contracts/suite.json` and
`contracts/baseline.json`. The order is the coordinator's and is in [trajectory.md](trajectory.md).
The three properties: implementable in this tree (no credential, no live call: recorded streams
only), no overlap (one unit), and it builds on nothing an open wave holds.

The move to `implemented` needs the acceptance evidence: the named tests of `llm-messages` for the
two failures and the two streams after text, the scenarios answered by `llm-conformance check` with
the raised `contracts/baseline.json`, and issue 83 closed with the release that carries the change.

## Decisions for the story's Open section

The story leaves the error codes open. The coordinator settled them before dispatch:

| Stop reason, no content block | Error code | Retried |
| --- | --- | --- |
| `model_context_window_exceeded` | `too-large` | never |
| `refusal` | `refused`, no cause named | never |
| any other unmodelled reason (`pause_turn`, `stop_sequence`, a reason in no list) | `unavailable`, marked retriable | before output only |

A terminal `message_delta` that follows no content block is not output for this failure: no block
opened, so the attempt has not started answering. `docs/design.md` "Provider failures retried before
output" and the retry row of the invariants table in `AGENTS.md` are amended in the same change, as
the story's second acceptance line asks. With a content block, every one of these reasons still
decodes to `StopReason::Incomplete { reason }`.

## Pre-flight

- Free disk on `/`: 52 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (`story:ess-pin-current-and-local-gate-finds-it` is wave 6).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:messages-empty-turn-with-an-unmodelled-stop-reason` | `impl/messages-empty-turn` | `llm-1125-empty` | `llm-1125-empty/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w30/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.
