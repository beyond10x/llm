# Wave 2026-11-24-rubric-judge

Status: **running**. Wave 1 of 3 of the [wave plan of 2026-11-24](trajectory.md). Approval:
`approval-record:wave-2026-11-24-rubric-judge` (the owner's order of 2026-11-24 to run the next wave
from the active stories, relayed by the operator session). Base: `main` at `c7c2a62cb` (llm 0.26.0).
Integration branch `wave/2026-11-24-rubric-judge`, tree `llm-1124-int`. Skill: AEP implementing 0.26.0.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-24, the lines naming
this wave's story:

```
wave 4
  story:rubric-judge (inferred)
collision: story:anthropic-access story:rubric-judge checks/conformance/src/decisions.rs (inferred)
collision: story:anthropic-access story:rubric-judge spec/domains/decisions.yaml (inferred)
collision: story:offline-replay-eval story:rubric-judge docs/design.md (inferred)
collision: story:openai-decisions-client story:rubric-judge checks/conformance/src/decisions.rs (inferred)
collision: story:openai-decisions-client story:rubric-judge contracts/decisions/scenarios (inferred)
collision: story:openai-decisions-client story:rubric-judge docs/design.md (inferred)
collision: story:openai-decisions-client story:rubric-judge spec/domains/decisions.yaml (inferred)
```

N is one. The story's scope is `inferred`; with one unit no pair runs at once. The three properties:
implementable in this tree (no credential, no live call), no overlap (one unit), one package
(`llm-decide`, with its conformance adapter and scenarios).

The move to `implemented` needs the acceptance evidence: the six scenarios answered by
`llm-conformance check` and the raised `contracts/baseline.json`.

## Pre-flight

- Free disk on `/`: 50 GB (`df -h`, 2026-11-24); the build pool reports its 100 GB floor as not met.
  The unit builds one tree.
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- No tree of an earlier wave holds a build directory (13 trees, 25 to 29 MB each).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own package's
tests and the conformance check; the whole `task check` runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:rubric-judge` | `impl/rubric-judge` | `llm-1124-judge` | `llm-1124-judge/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w27/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. The release 0.27.0 is a separate change after the wave closes.

## Cost

Filled at the close.
