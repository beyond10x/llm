---
format: aep.planning-md/3
id: story:replay-stops-at-the-last-case-have-scenarios
kind: story
status: draft
title: The replay stops at its last case are held by conformance scenarios
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w29-offline-replay-eval-pass-2
- decomposes: epic:routing
revision: 1
---
## Context

Wave 2026-11-24-offline-replay-eval added `replay` to `llm-calls`. Its correction round made a run
stop at the case that takes the spend past the cap, and at a case whose cost is not known, also when
that case is the last one of the corpus (`stopped` is `limit-exceeded` or `uncertain-spend`, and
`llm replay` exits 3).

`spec/domains/calls.yaml` states both stops in comments (lines 71 and 202 at the wave's close). No
scenario in `contracts/calls/scenarios/` holds them: the six scenarios of
`story:offline-replay-eval` cover the comparison, the cap at admission, a failed call, the digest,
the role and the label comparison. Only Rust tests hold the two stops
(`review-result:adversary-w29-offline-replay-eval-pass-2`, finding 6).

## Outcome

The two stops at the last case are behaviour the specification states and the conformance check
answers: a run whose last case takes the spend past its cap reports `limit-exceeded`, and a run
whose last case has no known cost reports `uncertain-spend`, each with every case sent and none
`not-sent`.

## Acceptance

- `contracts/calls/scenarios/` holds two scenarios, listed in `contracts/ess-inputs.yaml`:
  `a-replay-whose-last-case-passes-the-cap-reports-limit-exceeded` and
  `a-replay-whose-last-case-has-no-known-cost-reports-uncertain-spend`.
- `llm-conformance check` answers both through the real `replay`; `contracts/baseline.json` rises by
  the number of scenarios now answered and skips stay 0.
- Each scenario fails when the stop is read only on a later case's refusal (shown once by a mutant,
  recorded in the change's message).

## Verification

Targeted: `ess specify validate --path spec` and `llm-conformance check` with the pinned `ess`. No
live call.
