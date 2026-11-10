# Wave 2026-11-09-github-zero

Status: **open** (resumed 2026-11-10 after a pause of 2026-11-09). Approvals:
`approval-record:github-zero-acceptance-2026-11-09`, `approval-record:github-zero-wave-2026-11-09`,
`approval-record:github-zero-defects-2026-11-10` (all `non-interactive`). Epic
`epic:github-zero-2026-11-09`. Base: `main` at `7c56b19b` (llm 0.23.0). Integration branch
`epic/github-zero`, tree `llm-gh-zero-int`. One pull request carries the wave; no release is part of
it. Skill: AEP implementing 0.26.0.

## Plan

Derived with `aep plan artifact waves --kind story --status active --format json` on 2026-11-10
(aep 0.79.0), after the coordinator took `CHANGELOG.md`, `AGENTS.md` and the `story:durable-shadow-cap`
document rows out of the units' scope (below). The verb then places five of the epic's stories in
its wave 1 and `story:ingress-block-boundaries` in its wave 2; the ingress story collides only with
stories outside this epic (`story:anthropic-access`: `docs/implementation-status.md`,
`docs/messages.md`; `story:offline-replay-eval`, `story:openai-decisions-client`,
`story:rubric-judge`: `docs/design.md`), none of which run here, so all six run as one wave.
Unassessed: none. Cycles: none.

| Story | Objective | Route |
| --- | --- | --- |
| `story:routes-resolve-one-endpoint` (issue 36) | O1 | already on `main` since `4e18172f`; test output recorded; issue closed by the bot 2026-11-09 |
| `story:ingress-block-boundaries` | O3 | unit 1 |
| `story:bedrock-retry-invariant-held` | O1 | unit 2 |
| `story:opencode-go-session-on-every-spelling` | O3 | unit 3 |
| `story:call-tool-spawned-refusal-keeps-its-class` | O1 | unit 4 |
| `story:codex-renewal-blocking-io-bound` | O1 | unit 5 |
| `story:durable-shadow-cap` | O6 | unit 6 |

Not in the wave: `story:subscription-access-for-loom-consumers` (issue 13), held by
`credential-blocker:anthropic-live-qualification` and `credential-blocker:openai-subscription-account`.

## Shared files

`CHANGELOG.md`, `AGENTS.md`, `docs/implementation-status.md`, the generated status pages and, for
unit 6, `docs/design.md` are the coordinator's for units 2 to 6: each unit writes its lines as a
patch in its scratch directory and the coordinator applies them at merge, then regenerates. Unit 1
was dispatched before this rule and edits its own `CHANGELOG.md`, `docs/design.md` and
`docs/implementation-status.md` lines.

## Units

Up to four sub-agents at once: implementors `aep:implementor` (model sonnet), adversaries
`aep:adversary` (model opus). During the wave a unit runs only its own package's tests; the whole
`task check` runs once on `epic/github-zero` after every merge, and CI once on the pull request.

| Unit | Story | Branch | Tree | Head | Stage |
| --- | --- | --- | --- | --- | --- |
| 1 | `story:ingress-block-boundaries` | `impl/ingress-block-boundaries` | `llm-gh-zero-u1` | | implementing (opus, dispatched 2026-11-09) |
| 2 | `story:bedrock-retry-invariant-held` | `impl/bedrock-retry-invariant-held` | `llm-gh-zero-u2` | | planned |
| 3 | `story:opencode-go-session-on-every-spelling` | `impl/opencode-go-session-on-every-spelling` | `llm-gh-zero-u3` | | planned |
| 4 | `story:call-tool-spawned-refusal-keeps-its-class` | `impl/call-tool-spawned-refusal-keeps-its-class` | `llm-gh-zero-u4` | | planned |
| 5 | `story:codex-renewal-blocking-io-bound` | `impl/codex-renewal-blocking-io-bound` | `llm-gh-zero-u5` | | planned |
| 6 | `story:durable-shadow-cap` | `impl/durable-shadow-cap` | `llm-gh-zero-u6` | | planned |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`), building into
its own `target/`; every cargo call goes through the shared build slot. Scratch lives outside the
repository in the coordinator's scratch directory, one directory per unit.

## Commits this wave makes

The store commits; per unit its commits, the adversary's test commit and the merge into
`epic/github-zero`; the closing store commit; the squash merge of the one pull request into `main`.
No tag, no version bump, no release.
