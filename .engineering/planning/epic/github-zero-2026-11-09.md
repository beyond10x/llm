---
format: aep.planning-md/3
id: epic:github-zero-2026-11-09
kind: epic
status: active
title: Zero open GitHub issues and every store defect fixed, in one pull request
summary: 'Operator goal 2026-11-09: issues 13 and 36 and the open defect story, one epic, one PR'
relations:
- serves: vision:portable-model-inference
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-11-09T22:06:20Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-09T22:06:20Z", actor: "human:timo", revision: 3}
---
## Why

Operator goal, 2026-11-09, verbatim: "zero remaining issues on github, all bugs fixed from planning
store". Method, verbatim: "use /planning and /wave and /worktree:managing-worktrees - but in the end
provide 1 single PR to gh. make sure all are below 1 big epic for integration via bottoken from
./atlas".

## What it holds

Read on 2026-11-09 at `origin/main` `7c56b19b`. `gh issue list -R beyond10x/llm --state open`
returned two issues, 13 and 36. The store's non-terminal stories and open blockers were read for
ones that describe wrong behaviour.

| Item | Story | Route |
| --- | --- | --- |
| Issue 36, resolve one endpoint at a time | `story:routes-resolve-one-endpoint` | already on `main` since `4e18172f`; proven by test output, closed with that evidence |
| Issue 13, subscription access for Loom consumers | `story:subscription-access-for-loom-consumers` | held by `credential-blocker:anthropic-live-qualification` and `credential-blocker:openai-subscription-account`; the issue stays open |
| Defect: a relay refuses an answer whose blocks have no event between them | `story:ingress-block-boundaries` | implemented in the wave |

## Not in it, and why

Read as work that is not a defect (coordinator's reading, 2026-11-09):

- `story:durable-shadow-cap`: the cap is documented as per process; a durable cap is a capability.
- `story:responses-carries-refusal-text`, `story:cli-route-explain`, `story:cli-web-identity`,
  `story:cost-units-for-speech-and-requests`, `story:responses-websocket-steering`,
  `story:the-neutral-turn-in-ess`: capabilities that do not exist yet.
- `story:offline-replay-eval`, `story:openai-decisions-client`, `story:rubric-judge`: active
  capabilities accepted by the operator on 2026-11-05 for another wave.
- `dependency-blocker:connectors-arbitrary-secrets`: waits on another repository's contract.

## Acceptance

Every open GitHub issue of beyond10x/llm on 2026-11-09 and every open defect in this store has a
story under this epic, and each story is implemented, proven already fixed with recorded output, or
held by an open typed blocker that names what clears it. One pull request carries the wave. No
release is part of it.
