---
format: aep.planning-md/3
id: review-result:adversary-chat-pass-1
kind: review-result
status: archived
title: 'Adversary, chat projection, pass 1: needs-change on two blockers and a false coverage claim'
relations:
- reviews: story:chat-projection
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:34Z", actor: "human:timo", revision: 2}
---
## The pass

First adversarial pass over `story:chat-projection`, wave 1, against `wave1-chat` over base
`f63386e`. The unit had reported green: 43 Rust cases, 46 conformance scenarios passing three
times, 12 mutations each killing a named scenario.

Verdict: **needs-change**. Five cases added, four red. Six findings, all `introduced`. The
conformance lane stayed at 46 passing and the pass added no scenario — which is itself part of the
result, because none of the four defects is visible to that lane.

## The two blockers are one root cause

`StreamProjection::finish` can produce seven refusals. Two are explicitly stamped as dispatched;
the other five take the default, which says nothing was sent. All five are reachable only after the
entire response and its terminating sentinel have arrived. Dispatch is the retry signal the
contract document defines, and the cost library records it on every attempt, so a refusal that
happened after the request was served must not report that it never left.

The second half follows: the client's retention path returns the error unchanged whenever dispatch
says nothing was sent, so the first defect makes it discard the observation. Measured end to end
over a loopback socket, with the endpoint reporting forty prompt tokens and twelve completion
tokens, the error carries no observation at all. The doc comment two lines above that code says a
failure after dispatch keeps the last valid bound evidence.

The named fix is to stamp the remaining paths as dispatched, with one caution the pass supplies:
the contradictory-counters path re-validates any attached observation, so it must either carry a
snapshot that validates or carry the dispatch with no observation.

## The finding that matters beyond this unit

The unit's verification record states that it closed the absent-counter class rather than the
instance. Measured, that is false. A targeted mutation making the input counter default to zero
**survives all 43 Rust cases and all 46 scenarios** — 89 cases, zero failures — because no usage
object anywhere in the decode direction omits that field across five fixtures, fourteen scenarios
and nineteen Rust cases. Three more instances were added; the class is open on the decode side.

This is the third unit in the wave whose falsification record claims more coverage than it has, and
the first to claim explicitly that it had closed the class. The pass also found why it survived:
the client file is observed by no scenario and mutated by no falsification entry, and the record's
own "what this does not cover" section does not say so.

## Bound on what the pass touched

No implementation file was modified. The three source digests still equal the ones the unit
recorded in its own falsification file. The single worktree write is a new test file; the mutation
probe ran on a copy outside the worktree, and both the copy and its 1.2 GB build directory were
deleted.

## Report

```
unit: story:chat-projection
verdict: NEEDS-CHANGE
cases: executed 43→48, red 4 (conformance scenarios 46→46, red 0)
origin: introduced 6 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths, both deleted
needs-coordinator: none
```

```findings
- file: crates/llm-chat/src/incoming.rs
  line: 196
  category: contract-drift
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "five of the seven refusals the stream projection can produce carry the default dispatch saying nothing was sent, although all five are reachable only after the whole response and its sentinel arrived, while the two above them are explicitly stamped as dispatched."
- file: crates/llm-chat/src/client.rs
  line: 104
  category: mutant
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the retention path discards the bound observation whenever dispatch says nothing was sent, so a protocol refusal after dispatch loses the counters the endpoint reported, contradicting its own doc comment and the contract document."
- file: crates/llm-chat/src/incoming.rs
  line: 328
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the cache creation counter is hard-coded absent, so the extension field this crate's own ingress encoder writes is deleted on read-back, and the crate's documentation promises the opposite."
- file: docs/verification/chat.md
  line: 71
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the claim that the absent-counter-becomes-zero class was closed is false as measured: a targeted variant on the input counter survives all 43 Rust cases and all 46 scenarios, because no decode-direction usage object in the suite omits that field."
- file: docs/verification/chat.md
  line: 23
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the client file is observed by no scenario and mutated by no falsification entry, and the not-covered section does not say so, which is why the dispatch and evidence-retention defects survived a green suite."
- file: crates/llm-chat/src/ingress.rs
  line: 67
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the ingress decoder never runs the neutral request validation, so it accepts requests the unit's own outgoing projection refuses, and no case asserts the invalid result the adapter already exposes."
```

## What the pass attacked and could not break

Every counter other than the input one is covered absent-while-others-present, in both shapes, so a
blanket mutation on the counter readers dies. The encode direction is covered for every counter
including the input one. Chat ingress constructs no opaque item at all and refuses one regardless
of provenance, so the laundering defect found in a sibling unit is not present here. The sentinel
is genuinely required and a payload after it is refused. The pass re-derived which refusal fires
first for all nine refusal scenarios and found each fires for the reason it names — including the
three the unit had already rewritten for that exact problem. No refusal path interpolates request
or response bytes. All three recorded source digests match the shipped files.
