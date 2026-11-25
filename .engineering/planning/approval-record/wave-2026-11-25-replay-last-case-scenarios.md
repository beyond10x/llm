---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-replay-last-case-scenarios
kind: approval-record
status: accepted
title: Wave 2026-11-25-replay-last-case-scenarios authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:replay-stops-at-the-last-case-have-scenarios
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T15:55:11Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-replay-last-case-scenarios runs: wave 11 of the wave plan in
`docs/plan/trajectory.md`, one story, `story:replay-stops-at-the-last-case-have-scenarios`, one
unit.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. The story was accepted on 2026-11-25
(`approval-record:backlog-2026-11-25-seven-drafts-accepted`), with the order test defects first,
correctness second. No operator turn precedes the dispatch, so the stage-1 stop is recorded here
and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at the wave's
base (wave 10's opening commit `94db8e841` with `main` at LLM 0.33.0 merged):

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
  story:llm-decide-tests-read-no-clock-and-build-one-binary (inferred)
wave 2
  story:cli-route-explain (inferred)
  story:messages-cancellation-case-waits-on-a-signal
wave 3
  story:replay-stops-at-the-last-case-have-scenarios (inferred)
wave 4
  story:responses-carries-refusal-text (inferred)
wave 5
  story:test-binaries-one-per-crate (inferred)
wave 6
  story:tool-arguments-with-a-repeated-key (inferred)
6 wave(s), 133 collision(s), 0 unassessed
```

Check path: the verb, then the Scope section. The verb puts the story alone in its wave. N is one
story. The three properties: implementable in this tree (no credential, no live call: the
conformance target answers a replay with its own scripted upstream); one unit, so no two units
share a path; and the overlap with the two open waves is two documents only. Waves 9 and 10 change
test files; this story adds and edits none (the story's Scope, "Overlap"). All three touch
`AGENTS.md`, this story its line on the conformance floor alone. The wave is opened on wave 10's
opening commit so that the three rows of the trajectory are changed on one line of history, and it
merges after waves 9 and 10, the order the owner gave.

## What the coordinator settled before dispatch

- The change is data: two scenario files in `contracts/calls/scenarios/`, their two lines in
  `contracts/ess-inputs.yaml` after the existing replay lines, the regenerated
  `contracts/suite.json`, the floor in `contracts/baseline.json` (1090 to 1092, skips stay 0), the
  floor in `AGENTS.md`, and one CHANGELOG line that names the floor.
- No file under `spec/`, `checks/conformance/src/` or `crates/` changes. If a scenario cannot be
  written with what the target reads today, the unit stops and reports; it does not widen the
  target on its own.
- Each scenario asserts that every case was sent and none is `not-sent`, inside the run it reads
  (the story's Scope, second safety fact): without it the scenario passes when the stop is read
  only on a later case's refusal.
- The mutant of the third acceptance line (the summary of a run reads the stop from a refusal
  alone) is built in a managed worktree of its own, never committed, and its two failing scenario
  names go into the unit's commit message.
- The unit writes the two scenarios and the manifest lines first. The generated suite, the
  conformance check and the mutant run when the unit builds.
- One CHANGELOG line under `### Added`. The wave has no release of its own: the line rides the next
  release.

## Commits this authorises

The opening plan commit, the merge of `main` it starts from, the unit's commit and its corrections,
the merge into `wave/2026-11-25-replay-last-case-scenarios`, the closing store commit, and the merge
of that branch into `main` through one pull request. Nothing else.
