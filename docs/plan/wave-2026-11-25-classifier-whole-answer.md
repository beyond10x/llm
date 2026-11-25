# Wave 2026-11-25-classifier-whole-answer

Status: **open**. Wave 5 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-classifier-whole-answer` (the owner's order of 2026-11-24 to run
the next wave from the active stories, relayed by the operator session). Opened on the head of
wave 4, `299c4f906`, while pull request 100 waits for its merge; the integration branch takes `main`
before the unit merges. Integration branch `wave/2026-11-25-classifier-whole-answer`, tree
`llm-1125-clf-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: none. The story comes from a finding of wave 1's adversary
(`review-result:adversary-w27-rubric-judge-pass-1`): the classifier takes a cut, split or ambiguous
answer as a decision.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `299c4f906`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:classifier-takes-only-a-whole-answer (inferred)
wave 3
  story:ess-pin-current-and-local-gate-finds-it (inferred)
3 wave(s), 16 collision(s), 0 unassessed
```

N is one. The order is the trajectory's. `story:anthropic-access` is held by
`credential-blocker:anthropic-live-qualification`. The three properties: implementable in this tree
(no credential, no live call), no overlap (one unit), and it builds on wave 4's head, which changes
no file of `crates/llm-decide`.

The move to `implemented` needs the acceptance evidence: a test per case on the JSON-schema path
that fails before the change (a repeated `label`, an answer cut at the output limit, an answer in
two text items), a test per case of the class that reaches the forced-tool path, and one reader
serving the classifier and the judge.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The JSON-schema path's reader | `answer::whole`, the judge's; an answer it does not return abstains `OffLabel` |
| The forced-tool path | the call is taken only from a turn that stopped with `ToolCalls`; a call from a turn cut at the output limit or left incomplete abstains `OffLabel`; two calls abstain, with a test |
| A repeated key in tool arguments | not visible to `llm-decide` (the protocol decoders keep the last value); not in this wave: draft `story:tool-arguments-with-a-repeated-key` |
| The CHANGELOG | `### Fixed`, marked breaking: an answer taken as a decision before abstains now |

## Pre-flight

- Free disk on `/`: 63 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (`story:ess-pin-current-and-local-gate-finds-it` is wave 6).
- An adversary's mutant or base copy builds in a managed worktree (`AGENTS.md`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:classifier-takes-only-a-whole-answer` | `impl/classifier-whole-answer` | `llm-1125-clf` | `llm-1125-clf/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w31/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.
