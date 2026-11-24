# Wave 2026-11-24-offline-replay-eval

Status: **running**. Wave 3 of 3 of the [wave plan of 2026-11-24](trajectory.md). Approval:
`approval-record:wave-2026-11-24-offline-replay-eval` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Base: the head of wave 2,
`58a108db2` (pull request 96, `main` at 0.27.0 plus the decision endpoint). Integration branch
`wave/2026-11-24-offline-replay-eval`, tree `llm-1124-replay-int`. Skill: AEP implementing 0.26.0.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-24 after wave 2
closed:

```
wave 1
  story:anthropic-access (inferred)
wave 2
  story:offline-replay-eval (inferred)
collision: story:anthropic-access story:offline-replay-eval checks/conformance/src/calls.rs (inferred)
collision: story:anthropic-access story:offline-replay-eval spec/domains/calls.yaml (inferred)
unassessed: story:classifier-takes-only-a-whole-answer
unassessed: story:docs-ux-layout-checks-from-ux-browser
unassessed: story:ess-pin-current-and-local-gate-finds-it
unassessed: story:messages-empty-turn-with-an-unmodelled-stop-reason
2 wave(s), 2 collision(s), 4 unassessed
```

N is one. `story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification` and
is not in the plan; the four unassessed stories enter the plan after this wave. The story's scope is
`inferred`; with one unit no pair runs at once. The three properties: implementable in this tree (no
credential, no live call: scripted models and loopback servers), no overlap (one unit), and it
builds on what waves 1 and 2 added (the verdict's canonical JSON, the classifier's three paths).

The move to `implemented` needs the acceptance evidence: the six scenarios answered by
`llm-conformance check`, the raised `contracts/baseline.json`, and the `llm-cli` command test
`replay_prints_the_summary_and_exits_nonzero_past_the_cap`.

## Pre-flight

- Free disk on `/`: 52 GB (`df -h`, 2026-11-24). The unit builds one tree; the trees of wave 2 hold
  no build output.
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (`story:ess-pin-current-and-local-gate-finds-it`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:offline-replay-eval` | `impl/offline-replay-eval` | `llm-1124-replay` | `llm-1124-replay/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w29/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Cost

Filled at the close.
