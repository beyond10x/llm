---
format: aep.planning-md/3
id: approval-record:github-zero-wave-2026-11-09
kind: approval-record
status: approved
title: Wave 2026-11-09-github-zero proposed and run with no operator present
tags:
- non-interactive
relations:
- decides: story:ingress-block-boundaries
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-09T22:06:52Z", actor: "human:timo", revision: 2}
---
## Which stop

The stage-1 proposal of wave `2026-11-09-github-zero` (page `docs/plan/wave-2026-11-09-github-zero.md`).

## Why non-interactive

No operator turn until the goal is met (Stop hook). Goal, verbatim: "zero remaining issues on
github, all bugs fixed from planning store". The brief carrying it states the goal is the approval
of each wave.

## What was decided in the operator's absence

Wave 1 of 1 of the epic's plan. Units: `story:ingress-block-boundaries` (O3), one implementor
(`aep:implementor`) and one adversary (`aep:adversary`), both on model opus, one at a time, in
managed tree `llm-gh-zero-u1` on branch `impl/ingress-block-boundaries` forked from
`epic/github-zero`. `story:routes-resolve-one-endpoint` is closed on recorded test output, no unit.
Check path: `aep plan artifact waves` (aep 0.79.0). Verb lines for the wave's stories: wave 1 holds
`story:routes-resolve-one-endpoint`, wave 2 `story:ingress-block-boundaries`; collisions only with
stories outside this epic (`docs/implementation-status.md`, `docs/messages.md`, `docs/design.md`,
all inferred); unassessed none; cycles none. Pre-flight: 71G free on `/`, load about 38; every
cargo call through the shared build slot (3 slots, 20G floor).

Commits this bypass authorises: the opening store commit, the unit's commits, the adversary's test
commit, the merge into `epic/github-zero`, the closing store commit, and the one pull request's
squash merge into `main` through the bot. No tag, no version bump, no release.

## What the operator would have been asked

Whether to run this wave as proposed.
