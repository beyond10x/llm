---
format: aep.planning-md/3
id: approval-record:backlog-2026-11-24-four-drafts-accepted
kind: approval-record
status: accepted
title: The four drafts filed by the plan of 2026-11-24 are accepted and enter the plan after wave 3
relations:
- decides: story:messages-empty-turn-with-an-unmodelled-stop-reason
- decides: story:ess-pin-current-and-local-gate-finds-it
- decides: story:classifier-takes-only-a-whole-answer
- decides: story:docs-ux-layout-checks-from-ux-browser
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-24T16:24:30Z", actor: "human:timo", revision: 2}
---
## Decision

The four drafts the plan of 2026-11-24 filed are accepted and enter the plan after its wave 3:

- `story:messages-empty-turn-with-an-unmodelled-stop-reason` (issue 83)
- `story:ess-pin-current-and-local-gate-finds-it`
- `story:classifier-takes-only-a-whole-answer`
- `story:docs-ux-layout-checks-from-ux-browser`, which starts only when a release tag of the uxui
  repository that carries `ux-browser` exists

## Source

The coordinator put the question on 2026-11-24 with three options: A, accept all four; B, accept the
three defect stories; C, accept none. The operator session that acts with the owner's permissions
answered on 2026-11-24: "D2 = A, my decision: all 4 drafts enter the plan after wave 3; the
ux-browser story starts only when a uxui release tag exists."

## What it changes

Each of the four moves `draft` to `proposed` to `active`. None joins a running wave: the next plan
is derived with `aep plan artifact waves` when wave 3 of this plan is delivered.
