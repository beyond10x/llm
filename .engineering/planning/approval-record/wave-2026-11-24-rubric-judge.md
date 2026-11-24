---
format: aep.planning-md/3
id: approval-record:wave-2026-11-24-rubric-judge
kind: approval-record
status: accepted
title: Wave 2026-11-24-rubric-judge authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:rubric-judge
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-24T21:58:59Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-24-rubric-judge runs: wave 1 of 3 of the wave plan in `docs/plan/trajectory.md`, one
unit, `story:rubric-judge`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. The owner's words to it: "make sure they are building stuff and they have a plan and the
vision, a trajectory, a direction". No operator turn precedes the dispatch, so the stage-1 stop is
recorded here and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0), the lines naming the story:

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

Check path: the verb. Scope: inferred. Left the plan: `story:anthropic-access`, held by
`credential-blocker:anthropic-live-qualification`. Pre-flight: 50 GB free on `/`, shared compile
cache, one unit tree.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-24-rubric-judge`, the closing store commit, and the merge of that branch into `main`
through one pull request. Nothing else.
