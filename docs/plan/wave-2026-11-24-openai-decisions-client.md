# Wave 2026-11-24-openai-decisions-client

Status: **running**. Wave 2 of 3 of the [wave plan of 2026-11-24](trajectory.md). Approval:
`approval-record:wave-2026-11-24-openai-decisions-client` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Base: the head of wave 1,
`cc001ee87` (pull request 94, `main` at `58e27a4bb` plus the rubric judge). Integration branch
`wave/2026-11-24-openai-decisions-client`, tree `llm-1124-dec-int`. Skill: AEP implementing 0.26.0.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-24 after wave 1
closed, the lines naming this wave's story:

```
wave 3
  story:openai-decisions-client (inferred)
collision: story:anthropic-access story:openai-decisions-client checks/conformance/src/decisions.rs (inferred)
collision: story:anthropic-access story:openai-decisions-client crates/llm-core/src/vocabulary.rs (inferred)
collision: story:anthropic-access story:openai-decisions-client spec/domains/catalog.yaml (inferred)
collision: story:anthropic-access story:openai-decisions-client spec/domains/decisions.yaml (inferred)
collision: story:offline-replay-eval story:openai-decisions-client docs/design.md (inferred)
```

N is one. The story's scope is `inferred`; with one unit no pair runs at once. The three properties:
implementable in this tree (no credential, no live call; fixture bodies come from the provider's
published API description), no overlap (one unit), one new package (`llm-openai-decisions`) with the
classifier's third path in `llm-decide`, its conformance adapter and scenarios.

The move to `implemented` needs the acceptance evidence: the six scenarios answered by
`llm-conformance check` and the raised `contracts/baseline.json`.

## Pre-flight

- Free disk on `/`: 34 GB (`df -h`, 2026-11-24). The unit builds one tree; the wave 1 unit tree is
  removed.
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (wave 1's deviation; `story:ess-pin-current-and-local-gate-finds-it`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole `task check` runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:openai-decisions-client` | `impl/openai-decisions-client` | `llm-1124-decisions` | `llm-1124-decisions/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w28/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Cost

Filled at the close.
