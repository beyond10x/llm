---
format: aep.planning-md/2
id: review-result:adversary-responses-pass-1
kind: review-result
status: active
title: 'Adversary, responses projection, pass 1: needs-change on four blockers'
relations:
- reviews: story:responses-projection
revision: 1
---
## The pass

First adversarial pass over `story:responses-projection`, wave 1, against the unit's worktree
`wave1-responses` over base `f63386e`. The unit had reported green: 28 Rust cases, 43 conformance
scenarios passing three times, and 11 deliberate mutations each killing a named scenario.

Verdict returned: **needs-change**. Eleven cases added, six of them red. Nine findings, all
`introduced` — at the base commit the crate is a scaffold whose own documentation says it exports no
runtime API, so there is no prior behaviour any of these could have been inherited from.

## What the pass established that the unit's own suite could not

Six mutations the implementor had not tried, each applied to a scratch copy rather than the
worktree, and **all six survived the entire authored suite** — 43 scenarios and 28 Rust cases green
under every one:

| mutation | site |
|---|---|
| an absent input count becomes zero | `stream.rs:425` |
| the output-message content-part filter is removed | `stream.rs:372` |
| the ingress guard on provider-side storage is removed | `request.rs:136` |
| the ingress guard on non-streaming bodies is removed | `request.rs:128` |
| the incomplete-reason length bound widens from 64 to 6400 | `stream.rs:411` |
| the published-tool field guard is removed | `request.rs:280` |

The first of those is the repository's first-named defect — an unknown becoming a value — on the one
counter no fixture omits. The unit's own falsification record shows eleven mutations killed, which
reads as a suite that falsifies; four live guards are killed by nothing at all.

## Structural reason the ingress findings were invisible

`spec/domains/responses.yaml:60` gives `RequestProjection` a `request_preserved` fact and gives
`RequestIngestion` no re-projection fact to mirror it. No scenario can therefore observe an
ingress-to-egress disagreement, which is exactly the shape of two of the findings below. That is a
gap in the domain, not in the scenarios written against it.

## Bound on what the pass touched

No implementation file was modified, not even briefly. The two implementation files hash identical
to the implementor's own recorded pre-mutation hashes, and the newest file in the tree is the
adversary's own test file; every mutation ran on an `rsync` copy outside the worktree. The
conformance suite was run but not changed.

## Report

```
unit: story:responses-projection
verdict: NEEDS-CHANGE
cases: executed 28→39, red 6
origin: introduced 9 / pre-existing 0 / undecided 0
wrote-outside-worktree: 12 paths under home-path:sha256:1d592bbb54651136f6d5c82363e7ba691e7d986db5e6b0bc6bf04beadace26f1
needs-coordinator: none
```

```findings
- file: crates/llm-responses/src/stream.rs
  line: 197
  category: acceptance
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "every refusal raised while decoding the terminal object's own output is returned with no observation, so a turn refused for an unreadable tool call loses the usage the terminal object reported in the same breath."
- file: crates/llm-responses/src/request.rs
  line: 283
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "ingress accepts `strict` inside a published tool, never reads it, and re-projects `strict: false`, silently inverting a flag the doc says would be refused rather than dropped."
- file: crates/llm-responses/src/request.rs
  line: 434
  category: property
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "ingress stamps every unmodelled entry with the reading binding, so opaque state a repointed binding refuses on egress is reinstated as native and sent after one pass through ingest_request."
- file: crates/llm-responses/src/stream.rs
  line: 425
  category: mutant
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "an absent input_tokens turned into zero survives all 43 scenarios and all 28 authored Rust cases, leaving the repository's first-named defect unguarded on the one counter no fixture omits."
- file: crates/llm-responses/src/stream.rs
  line: 372
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "an output-message content part outside output_text/text is dropped with no refusal, no warning and no opaque item, leaving an empty assistant turn that reports EndTurn."
- file: crates/llm-responses/src/request.rs
  line: 184
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "ingress applies no tool-name class check, so a gateway accepts a request its own egress refuses to forward, against the doc's claim that both directions disagree about nothing."
- file: docs/verification/responses-falsification.json
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "four live guards — store, stream, the published-tool field subset and the 64-byte incomplete-reason bound — are killed by no scenario and no Rust case, so the falsification record overstates what the suite checks."
- file: spec/domains/responses.yaml
  line: 60
  category: judgement
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "RequestIngestion carries no re-projection fact to mirror request_preserved, so no scenario can observe an ingress-to-egress disagreement at all."
- file: crates/llm-responses/src/stream.rs
  line: 383
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "stop_reason trusts the status string rather than the terminal event name, so a response.incomplete without status decodes as EndTurn; I could not show a server that omits it."
- file: crates/llm-responses/src/request.rs
  line: 245
  category: mutant
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "check_tool_names implements the documented pattern as `*` rather than `+`, accepting an empty name vacuously; unreachable only because ToolName refuses empty first."
```

## What the pass attacked and could not break

Recorded because it is the half that says where the unit is solid: the six-coordinate opaque-state
refusal on the egress path; the unreported model never substituted by the configured one on either
the terminal or the failed path; upstream provider text never reaching a diagnostic on any of three
paths; contradictory counters refused with the observation attached; the top-level body-field
subset; and the two class-level tests that keep a declared constant and its handler in step, which
the pass reports it could not desynchronise.
