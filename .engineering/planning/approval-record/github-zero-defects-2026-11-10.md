---
format: aep.planning-md/3
id: approval-record:github-zero-defects-2026-11-10
kind: approval-record
status: approved
title: Five more store defects accepted into the github-zero wave with no operator present
tags:
- non-interactive
relations:
- decides: story:bedrock-retry-invariant-held
- decides: story:opencode-go-session-on-every-spelling
- decides: story:call-tool-spawned-refusal-keeps-its-class
- decides: story:codex-renewal-blocking-io-bound
- decides: story:durable-shadow-cap
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-10T00:19:23Z", actor: "human:timo", revision: 2}
---
## Which stop

Acceptance of five more defect stories into `epic:github-zero-2026-11-09`, and the wave that holds
them. Accepting a story is the operator's act; this run has no operator turn.

## Why non-interactive

The run is held by a Stop hook until the goal is met; no operator turn. Goal, verbatim: "zero
remaining issues on github, all bugs fixed from planning store". Operator, 2026-11-10, relayed by the
coordinating session, verbatim: "do work on llm, inference, connectors - start 3 opus 5.5
/aep:implementing agents - each processing a whole wave of up to 10 issues (the more the better) -
run no expensive gates in the middle, only at the end of the wave". Brief rule 12: one wave of up to
10 stories, implementors run only their package's tests, the full gate once at the end.

## What was decided in the operator's absence

The `aep:plan-reviewer` triage of 2026-11-10 (read-only, opus) found four open defects no story
fixed; each was filed as a draft story citing its source and is accepted here, together with the
existing draft `story:durable-shadow-cap` (a money cap a restart resets, from
`review-result:security-seams-u4-pass-1`):

- `story:bedrock-retry-invariant-held` (an AGENTS.md invariant marked UNMAPPED)
- `story:opencode-go-session-on-every-spelling` (`review-result:adversary-w19-opencode-go-session-pass-1` F1; nothing found that reaches it)
- `story:call-tool-spawned-refusal-keeps-its-class` (an ignored adversary case)
- `story:codex-renewal-blocking-io-bound` (`review-result:adversary-w10-codex-test-pass-2`; acceptance amended from the scoper's trace)
- `story:durable-shadow-cap`

Each moves draft -> proposed -> active. They join the one wave of the epic beside
`story:ingress-block-boundaries`, one implementor each (`aep:implementor`, model sonnet), one
adversary each (`aep:adversary`, opus), up to four sub-agents at once, package tests only during the
wave, the full `task check` once on `epic/github-zero` after every merge.

Commits this bypass authorises: per unit its commits and the adversary's test commit and the merge
into `epic/github-zero`; the closing store commit; the one pull request's squash merge into `main`
through the bot. No tag, no version bump, no release.

## What the operator would have been asked

Whether these five are defects worth this wave, given that the wave normally takes no finding
draft before the plan is delivered (the operator's 2026-11-10 instruction asks for as many as fit).
