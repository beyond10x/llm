---
format: aep.planning-md/3
id: approval-record:wave-2026-11-24-offline-replay-eval
kind: approval-record
status: accepted
title: Wave 2026-11-24-offline-replay-eval authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:offline-replay-eval
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-24T21:00:46Z", actor: "human:timo", revision: 2}
---
## Decision

Wave 2026-11-24-offline-replay-eval runs: wave 3 of 3 of the wave plan in
`docs/plan/trajectory.md`, one unit, `story:offline-replay-eval`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Wave 2 is delivered (`docs/plan/wave-2026-11-24-openai-decisions-client.md`, pull
request 96). No operator turn precedes the dispatch, so the stage-1 stop is recorded here and the
wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) after wave 2 closed:

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

Check path: the verb. Scope: inferred. Left the plan: `story:anthropic-access`, held by
`credential-blocker:anthropic-live-qualification`. The four unassessed stories were accepted on
2026-11-24 to enter the plan after this wave; none runs beside it. Pre-flight: 52 GB free on `/`,
shared compile cache, one unit tree.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-24-offline-replay-eval`, the closing store commit, and the merge of that branch into
`main` through one pull request. Nothing else.
