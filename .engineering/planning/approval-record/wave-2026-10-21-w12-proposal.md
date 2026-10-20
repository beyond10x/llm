---
format: aep.planning-md/3
id: approval-record:wave-2026-10-21-w12-proposal
kind: approval-record
status: draft
title: Wave 2026-10-21-w12 authorised by the owner's directed-session grant to b10x-chef-orchestrator
relations:
- decides: story:timings-before-the-last-chunk
revision: 2
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-21-w12.

## What authorises passing it

Session `b10x-chef-orchestrator` approved on 2026-10-21, under the owner's directed-session grant
(exception 6 of the standing commit exceptions): "a wave with story:timings-before-the-last-chunk
only (push of the plan branch included), then release 0.8.4 through your process on ess 0.66.0,
landing by bot fast-forward."

Covered: the commits the wave page lists; bot pushes of the plan and wave branches; the wave pull
request and its landing on `main` by a bot fast-forward of the green head; release `0.8.4` through
AGENTS.md "Release" on ess 0.66.0, landed the same way, with its tag and GitHub Release. Not
covered: registry publishing, and every other story. One agent at a time; every cargo command goes
through `b10x-build-slot`.

## The units, and the objective each serves

One unit, `story:timings-before-the-last-chunk`, serving `vision:portable-model-inference`. Its
`## Design decisions` records the decision the proposal named.
