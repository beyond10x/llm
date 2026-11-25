---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-ess-pin
kind: approval-record
status: accepted
title: Wave 2026-11-25-ess-pin authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:ess-pin-current-and-local-gate-finds-it
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T03:21:06Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-ess-pin runs: wave 6 of the wave plan in `docs/plan/trajectory.md`, one unit,
`story:ess-pin-current-and-local-gate-finds-it`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Wave 5 is delivered (`docs/plan/wave-2026-11-25-classifier-whole-answer.md`, pull
request 102). No operator turn precedes the dispatch, so the stage-1 stop is recorded here and the
wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at the head of
wave 5 (`857979d5d`):

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:ess-pin-current-and-local-gate-finds-it (inferred)
2 wave(s), 6 collision(s), 0 unassessed
```

Check path: the verb. Scope: inferred. The order is the trajectory's: the ESS pin after the two
stories that added scenarios, so the suite and the schema files are regenerated once; the docs UX
checks after it. `story:anthropic-access` stays out under
`credential-blocker:anthropic-live-qualification`. The three properties: implementable in this tree
(no credential, no live call), no overlap (one unit), and it builds on wave 5's head, which is in
pull request 102. Pre-flight: 54 GB free on `/`, shared compile cache, one unit tree.

## What the coordinator settled before dispatch

- The pin moves to ESS 0.99.0, the release marked latest on 2026-11-25 (published 2026-11-24). The
  digest of `ess-0.99.0-x86_64-unknown-linux-gnu.tar.gz` in that release's `SHA256SUMS` is
  `238d64266e3014ccf98e190b6525433c90686b6ef9c9a5cd5dd1a852cb4fdf32`.
- The story leaves open whether the local gate finds the pinned `ess` or refuses. Both: the
  conformance step uses a pinned `ess` it finds under `target/`, fetches it there with the digest
  check `gate.yml` makes when it is missing, and uses the `ess` on `PATH` only when it reports the
  pinned version. When none of these gives the pinned version, it names the version it found and
  the version it needs and exits non-zero before it compares a projection.
- `task check` runs `ess specify validate` one step before the conformance step. It goes through
  the same resolution, so no step of `task check` runs another `ess`.
- One test holds that `gate.yml`, `checks/conformance/Cargo.toml` and the documents name one ESS
  version, as the gate's budget test does for the nextest pin.
- The fetch is a network request of the gate, as the "Install ess" step of CI is. The test step
  makes none.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-25-ess-pin`, the closing store commit, and the merge of that branch into `main`
through one pull request. Nothing else.
