---
format: aep.planning-md/3
id: approval-record:wave-2026-11-24-openai-decisions-client
kind: approval-record
status: accepted
title: Wave 2026-11-24-openai-decisions-client authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:openai-decisions-client
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-24T16:24:30Z", actor: "human:timo", revision: 2}
---
## Decision

Wave 2026-11-24-openai-decisions-client runs: wave 2 of 3 of the wave plan in
`docs/plan/trajectory.md`, one unit, `story:openai-decisions-client`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Wave 1 is delivered (`docs/plan/wave-2026-11-24-rubric-judge.md`, pull request 94). No
operator turn precedes the dispatch, so the stage-1 stop is recorded here and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) after wave 1 closed, the lines
naming the story:

```
wave 3
  story:openai-decisions-client (inferred)
collision: story:anthropic-access story:openai-decisions-client checks/conformance/src/decisions.rs (inferred)
collision: story:anthropic-access story:openai-decisions-client crates/llm-core/src/vocabulary.rs (inferred)
collision: story:anthropic-access story:openai-decisions-client spec/domains/catalog.yaml (inferred)
collision: story:anthropic-access story:openai-decisions-client spec/domains/decisions.yaml (inferred)
collision: story:offline-replay-eval story:openai-decisions-client docs/design.md (inferred)
```

Check path: the verb. Scope: inferred. Left the plan: `story:anthropic-access`, held by
`credential-blocker:anthropic-live-qualification`. Pre-flight: 34 GB free on `/`, shared compile
cache, one unit tree.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-24-openai-decisions-client`, the closing store commit, and the merge of that branch
into `main` through one pull request. Nothing else.
