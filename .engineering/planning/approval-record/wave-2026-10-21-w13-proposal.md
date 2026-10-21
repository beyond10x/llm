---
format: aep.planning-md/3
id: approval-record:wave-2026-10-21-w13-proposal
kind: approval-record
status: draft
title: Wave 2026-10-21-w13 authorised by the owner's directed-session grant to b10x-chef-orchestrator
relations:
- decides: story:review-2026-10-21-ingress-strictness
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-21-w13.

## What authorises passing it

Session `b10x-chef-orchestrator` approved on 2026-10-21, under the owner's directed-session grant
(exception 6 of the standing commit exceptions), one wave with one unit,
`story:review-2026-10-21-ingress-strictness`, on three conditions:

1. "Before the PR, grep the beyond10x consumers (inference, loom, takt, ekr, metaharness, uilab,
   tools) for tools without parameters and text parts without text. Report the counts. If any
   consumer sends either, stop and tell me; that is a consumer break and not mine to approve."
2. "PR through the bot route, fast-forward on green only."
3. "The 0.8.5 release is not approved here. Propose it with the ess pin move after the merge."

Condition 1 was reported (0 real cases) and accepted by the same session before the pull request.

Covered: the commits the wave page lists; bot pushes of the wave branch; the wave pull request and
its landing on `main` by a bot fast-forward of the green head. Not covered: a release, registry
publishing, and every other story.

## The units, and the objective each serves

One unit, `story:review-2026-10-21-ingress-strictness`, serving `vision:portable-model-inference`.
