# Wave 2026-11-09-github-zero

Status: **open** (`approval-record:github-zero-acceptance-2026-11-09`, `non-interactive`). Epic
`epic:github-zero-2026-11-09`. Base: `main` at `7c56b19b` (llm 0.23.0). Integration branch
`epic/github-zero`, tree `llm-gh-zero-int`. One pull request carries the wave; no release is part of
it. Skill: AEP implementing 0.26.0.

## Plan

Derived with `aep plan artifact waves --kind story --status active --format json` on 2026-11-09.
The verb placed the epic's stories in its waves 1 and 2; the other stories it placed
(`story:anthropic-access`, `story:offline-replay-eval`, `story:openai-decisions-client`,
`story:rubric-judge`) are not part of this epic and are not run here.

| Order | Story | Objective | Route |
| --- | --- | --- | --- |
| 1 | `story:routes-resolve-one-endpoint` (issue 36) | O1 | already on `main` since `4e18172f`; test output recorded, nothing built |
| 2 | `story:ingress-block-boundaries` | O3 | one unit, `impl/ingress-block-boundaries` |

Collisions the verb printed for the wave's stories, all `inferred` and all with stories outside
this epic: `docs/implementation-status.md` and `docs/messages.md` with `story:anthropic-access`;
`docs/design.md` with `story:offline-replay-eval`, `story:openai-decisions-client` and
`story:rubric-judge`. Unassessed: none. Cycles: none.

Not in the wave: `story:subscription-access-for-loom-consumers` (issue 13), held by
`credential-blocker:anthropic-live-qualification` and `credential-blocker:openai-subscription-account`.

## Units

One unit tree at a time; at most one sub-agent runs at once.

| Story | Branch | Head | Tree | Stage |
| --- | --- | --- | --- | --- |
| `story:ingress-block-boundaries` | `impl/ingress-block-boundaries` | | `llm-gh-zero-u1` | planned |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`), building into
its own `target/`; every cargo call goes through the shared build slot. Scratch lives outside the
repository in the coordinator's scratch directory.

## Commits this wave makes

The opening store commit; one or more unit commits and the adversary's test commit; the merge into
`epic/github-zero`; the closing store commit; the squash merge of the one pull request into `main`.
No tag, no version bump, no release.
