---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-messages-empty-turn
kind: approval-record
status: accepted
title: Wave 2026-11-25-messages-empty-turn authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:messages-empty-turn-with-an-unmodelled-stop-reason
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-24T23:47:59Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-messages-empty-turn runs: wave 4 of the wave plan in `docs/plan/trajectory.md`
(the first of the four stories accepted on 2026-11-24), one unit,
`story:messages-empty-turn-with-an-unmodelled-stop-reason`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Wave 3 is delivered (`docs/plan/wave-2026-11-24-offline-replay-eval.md`, pull
request 98). No operator turn precedes the dispatch, so the stage-1 stop is recorded here and the
wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, after the four
stories got their Scope sections:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:classifier-takes-only-a-whole-answer (inferred)
wave 3
  story:ess-pin-current-and-local-gate-finds-it (inferred)
wave 4
  story:messages-empty-turn-with-an-unmodelled-stop-reason (inferred)
4 wave(s), 38 collision(s), 0 unassessed
```

Check path: the verb. Scope: inferred. Every pair of the four stories collides on `AGENTS.md` and
`CHANGELOG.md`, so each wave holds one story. Left the plan: `story:anthropic-access`, held by
`credential-blocker:anthropic-live-qualification`. The order differs from the verb's and is the
coordinator's: this story first, because issue 83 is the one gap a caller of the library reported;
then the classifier; then the ESS pin, which regenerates the suite once after the two stories that
add scenarios; then the docs UX checks, which wait on another repository's release tag. No
`depends_on` is crossed by that order. Pre-flight: 52 GB free on `/`, shared compile cache, one unit
tree.

## What the coordinator settled for the story's Open section

- `model_context_window_exceeded` without content: `too-large`, never retried.
- `refusal` without content: `refused`, never retried, no cause named.
- Every other unmodelled reason without content, listed by the provider or not: `unavailable`,
  marked retriable. A terminal `message_delta` that follows no content block is not output for this
  failure; `docs/design.md` "Provider failures retried before output" and the retry row of the
  invariants table in `AGENTS.md` are amended by this story, as its second acceptance line asks.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-25-messages-empty-turn`, the closing store commit, and the merge of that branch into
`main` through one pull request. Nothing else.
