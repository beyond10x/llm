# Wave 2026-11-25-ess-pin

Status: **open**. Wave 6 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-ess-pin` (the owner's order of 2026-11-24 to run the next wave from
the active stories, relayed by the operator session). Opened on the head of wave 5, `857979d5d`,
while pull request 102 waits for its merge; the integration branch takes `main` before the unit
merges. Integration branch `wave/2026-11-25-ess-pin`, tree `llm-1125-ess-int`. Skill: AEP
implementing 0.26.0.

Consumer gap moved: none. The story comes from wave 1: the local gate passed the conformance check
only with a hand-carried `ess` 0.87.0, while the workstation and the build node carry a newer one.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `857979d5d`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:ess-pin-current-and-local-gate-finds-it (inferred)
2 wave(s), 6 collision(s), 0 unassessed
```

N is one. The order is the trajectory's. `story:anthropic-access` is held by
`credential-blocker:anthropic-live-qualification`. The three properties: implementable in this tree
(no credential, no live call), no overlap (one unit), and it builds on wave 5's head.

The move to `implemented` needs the acceptance evidence: `task check` on a clean checkout passing
with the `ess` version `gate.yml` names, locally and in CI; with another `ess` version on `PATH` the
conformance step naming both versions and exiting non-zero before it compares a projection;
`contracts/baseline.json` not fallen and no scenario skipped.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The version | ESS 0.99.0, the release marked latest on 2026-11-25 |
| Finds it, or refuses | both: a pinned `ess` under `target/` is used; when it is missing it is fetched there with the digest check `gate.yml` makes; the `ess` on `PATH` is used only when it reports the pinned version; otherwise the step names the version found and the version needed and exits non-zero before a projection is compared |
| `ess specify validate` in `task check` | goes through the same resolution |
| One version in every place | a test holds that `gate.yml`, `checks/conformance/Cargo.toml` and the documents name one ESS version |

## Pre-flight

- Free disk on `/`: 54 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The workstation's `ess` is 0.98.0 and the build node's is 0.99.0 (the conformance step of wave 5's
  gate printed it).
- An adversary's mutant or base copy builds in a managed worktree (`AGENTS.md`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:ess-pin-current-and-local-gate-finds-it` | `impl/ess-pin` | `llm-1125-ess` | `llm-1125-ess/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w32/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.
