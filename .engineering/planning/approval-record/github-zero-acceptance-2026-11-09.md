---
format: aep.planning-md/3
id: approval-record:github-zero-acceptance-2026-11-09
kind: approval-record
status: approved
title: 'Epic github-zero: stories accepted with no operator present'
tags:
- non-interactive
relations:
- decides: epic:github-zero-2026-11-09
- decides: story:ingress-block-boundaries
- decides: story:routes-resolve-one-endpoint
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-09T22:06:24Z", actor: "human:timo", revision: 2}
---
## Which stop

Acceptance of the stories under `epic:github-zero-2026-11-09`, and of the epic itself. Accepting a
story is the operator's act (AEP implementing, wave.md); this run has no operator turn.

## Why non-interactive

The operator started the run on 2026-11-09 with a goal that a Stop hook enforces and no operator turn
until it is met. The goal, verbatim: "zero remaining issues on github, all bugs fixed from planning
store". Method, verbatim: "use /planning and /wave and /worktree:managing-worktrees - but in the end
provide 1 single PR to gh. make sure all are below 1 big epic for integration via bottoken from
./atlas". The brief that carries it (rule 3) states that the goal is the acceptance of these stories
and the approval of each wave, and asks that each bypassed stop be recorded here.

## What was decided in the operator's absence

- `epic:github-zero-2026-11-09` moves draft -> proposed -> active.
- `story:ingress-block-boundaries` moves draft -> proposed -> active, for the wave. Its fix adds a
  variant to `llm_core::StreamEvent`, which is not `#[non_exhaustive]`, so it is a **breaking**
  change to the neutral boundary. The 0.20.0 precedent (`RefusalDelta`) had its own operator
  approval; this one was chosen by the coordinator without one, because the story's acceptance
  cannot pass on the encoder-only route (scope section of the story). The CHANGELOG carries the
  `**Breaking:**` line under Unreleased. No release is cut.
- `story:routes-resolve-one-endpoint` (issue 36) moves draft -> proposed -> active, so that recorded
  test output can move it to implemented. Nothing is built for it.
- `story:subscription-access-for-loom-consumers` (issue 13) is NOT accepted. It stays draft, held by
  `credential-blocker:anthropic-live-qualification` and `credential-blocker:openai-subscription-account`;
  issue 13 stays open.

## What the operator would have been asked

Whether to accept these two stories into one wave, and whether the breaking `StreamEvent` change is
acceptable for an unreleased main (the alternative being to leave the defect open).
