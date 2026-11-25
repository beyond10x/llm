---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-classifier-whole-answer
kind: approval-record
status: accepted
title: Wave 2026-11-25-classifier-whole-answer authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:classifier-takes-only-a-whole-answer
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T02:05:51Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-classifier-whole-answer runs: wave 5 of the wave plan in `docs/plan/trajectory.md`,
one unit, `story:classifier-takes-only-a-whole-answer`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Wave 4 is delivered (`docs/plan/wave-2026-11-25-messages-empty-turn.md`, pull
request 100). No operator turn precedes the dispatch, so the stage-1 stop is recorded here and the
wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at the head of
wave 4 (`299c4f906`):

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

Check path: the verb. Scope: inferred. The order is the trajectory's: this story before the ESS pin,
which regenerates the suite once after the stories that add scenarios, and before the docs UX
checks. `story:anthropic-access` stays out under `credential-blocker:anthropic-live-qualification`.
The three properties: implementable in this tree (no credential, no live call), no overlap (one
unit), and it builds on wave 4's head, which is in pull request 100 and changes no file of
`crates/llm-decide`. Pre-flight: 63 GB free on `/`, shared compile cache, one unit tree.

## What the coordinator settled before dispatch

- JSON-schema path: the answer is read by `answer::whole`, the judge's reader. An answer it does not
  return abstains `OffLabel`.
- Forced-tool path: the call is taken only from a turn that stopped with `ToolCalls`. A call from a
  turn cut at the output limit or left incomplete abstains `OffLabel`. Two calls abstain, as today,
  now with a test.
- A repeated key in tool arguments cannot be seen by `llm-decide`: the protocol decoders parse the
  arguments and keep the last value. That is not in this wave; it is the draft
  `story:tool-arguments-with-a-repeated-key`.
- An answer that was taken as a decision before and abstains now is a changed result for the same
  bytes: the CHANGELOG line is marked breaking.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-25-classifier-whole-answer`, the closing store commit, and the merge of that branch
into `main` through one pull request. Nothing else.
