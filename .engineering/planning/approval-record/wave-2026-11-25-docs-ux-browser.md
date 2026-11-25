---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-docs-ux-browser
kind: approval-record
status: accepted
title: Wave 2026-11-25-docs-ux-browser authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:docs-ux-layout-checks-from-ux-browser
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T07:47:22Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-docs-ux-browser runs: wave 7 of the wave plan in `docs/plan/trajectory.md`, one unit,
`story:docs-ux-layout-checks-from-ux-browser`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Wave 6 is delivered (`docs/plan/wave-2026-11-25-ess-pin.md`, pull request 105). No
operator turn precedes the dispatch, so the stage-1 stop is recorded here and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at
`8f3abb719`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
unassessed: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
1 wave(s), 0 collision(s), 1 unassessed
```

Check path: the verb. Scope: inferred. `story:anthropic-access` stays out under
`credential-blocker:anthropic-live-qualification`. The three properties: implementable in this tree
(no credential, no live call; one pinned download from a public release), no overlap (one unit),
and it builds on the ESS pin wave, whose resolver it reuses. The story's "Open" line is closed: the
uxui repository published release 0.8.1 on 2026-11-25 with the asset
`ux-browser-0.8.1-x86_64-unknown-linux-gnu.tar.gz`, SHA-256
`2080904f1e34dfab904a1d35dabf6b50b30b58bb0ee0b0340f2aec447ea2029e` (the release's digest field,
read by the coordinator). Pre-flight: 60 GB free on `/`, shared compile cache, one unit tree.

## What the coordinator settled before dispatch

- The pin is uxui 0.8.1, not the 0.3.0 the offered patch names. Its one home is a step
  "Install ux-browser" in the job "Docs UX gate" of `docs-ux.yml`, with the version and the SHA-256
  in the step's `env`, written like the step "Install ess" of `gate.yml`. The local task reads that
  step through the resolver of `checks/conformance/src/ess.rs`, made general over the tool.
- `ux-browser` 0.8.1 has no `--version`. A fetched asset is proved by its digest, and after each
  run the gate compares the version the evidence names (`conditions.tools.ux-browser`) with the
  pin. A `ux-browser` on `PATH` is not used locally. The uxui session was asked for a `--version`.
- One bound of 120 s over the whole UX step: `ux-browser` runs inside the Playwright run when the
  first measurement shows the run's `globalTimeout` covers it; otherwise the implementor reports
  before another bound is built.
- The round takes its five values from `ux-browser` too, with a second configuration file for the
  round's viewports. `ux/rounds/R-01/` is not regenerated.
- The Chrome build is pinned in both configuration files to the build the harness's Playwright
  installs, and a test holds the two equal.
- The offered patch is a starting point. Every number in the wave's report is measured in this
  wave: the 135 values on one build, the planted defects, the step's wall time at 4 and at 2
  workers before and after.
- Left out of the patch: the go-task install in CI and `Taskfile.yml` in the workflow's path
  filter. The idle wait before axe goes in only if the flake it answers reproduces here.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-25-docs-ux-browser`, the closing store commit, and the merge of that branch into `main`
through one pull request. Nothing else.
