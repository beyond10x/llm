---
format: aep.planning-md/3
id: review-result:adversary-chat-pass-2
kind: review-result
status: archived
title: 'Adversary, chat projection, pass 2: two blockers, the entry-point enumeration missed a third'
relations:
- reviews: story:chat-projection
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:34Z", actor: "human:timo", revision: 2}
---
## The pass

Second and final adversarial pass over `story:chat-projection`, wave 1. The attack budget is spent.

Verdict: **needs-change**. Six cases added, all six red. Four findings, all `introduced`. No
mutation was applied to any file; every probe is expressed inside a case. The four recorded source
digests still match the shipped files. The pass declined to run the unit's falsification harness
because that harness edits production files in place, and said so.

## The same shape that beat the sibling projection, at a different site

Pass 1 found that refusals reachable only after the endpoint had already sent bytes were reporting
that nothing was sent. The unit answered it as a class: apply the correction once at each public
entry point, so a refusal added inside cannot escape it. Its documentation, its source comment and
its own structural test all say there are two entry points.

There are three. The streamed-event accept path is the third, and it is the one without the
correction, so six refusals reachable from it still report that nothing was sent.

Worse, the client reads a refusal's own not-sent marker as proof that nothing was dispatched and
hands the error straight back. Inside a turn, every error after the request has been posted is
after dispatch, whatever marker it carries. Three independent callers reach it: a bounded sink
refusing an event, outcome validation refusing a model's unpublished tool call, and the third entry
point above. The transport stamps every error it produces as dispatched, so that branch has no
legitimate caller at all — it fires only because of those three.

The unit's own comment two lines above says the opposite happens.

This is the second unit in this wave to fix an evidence-after-dispatch defect by enumerating entry
points and miss one. In both cases the enumeration was correct about what it enumerated.

## Report

```
unit: story:chat-projection
verdict: NEEDS-CHANGE
cases: executed 52→58, red 6
origin: introduced 4 / pre-existing 0 / undecided 0
wrote-outside-worktree: 1 path
needs-coordinator: the conformance lane cannot observe the stream accept path or the client without an implementation file
```

```findings
- file: crates/llm-chat/src/client.rs
  line: 118
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the retention path reads a refusal's own not-sent marker as proof nothing was dispatched, so a sink failure, an outcome-validation failure and a mid-stream refusal all leave the client reporting not-sent with no evidence for a turn that was served, streamed and may already be billed."
- file: crates/llm-chat/src/incoming.rs
  line: 61
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the streamed-event accept path is the third public entry point of the decode direction and the one that does not apply the correction, so six refusals reachable only from bytes an endpoint already served carry not-sent, contradicting the documentation, the source comment and the structural test that all say there are two entry points."
- file: crates/llm-chat/src/incoming.rs
  line: 299
  category: boundary
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "the single-choice guard reads an absent or non-integer index as agreement with index zero, so two choices in one chunk are concatenated into one assistant turn rather than refused as the documentation promises; the chunk was constructed and no server was shown to emit one."
- file: crates/llm-chat/src/ingress.rs
  line: 553
  category: property
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "the absent-counter rule was made a rule on the decode side only; the encode side is covered by exact-equality instances spanning today's five counters, so a sixth counter would be silently dropped on egress with nothing failing, and it cannot be shown red without adding a field to a frozen crate."
```

## What the pass verified and could not break

The ingress validation decision is sound: the outgoing validation calls the plain validation as its
first line and only adds checks on top, so ingress is a strict subset and cannot refuse something
the outgoing path accepts. The unit's own new survivor case drives the real client over a real
socket rather than a stub, and the attachment it guards is the only one of its kind in the crate.
The absent-counter rule holds on the decode side, including the sixth-counter question: the
complete report's key set is asserted against the table, so a new counter forces the table to grow.
Transport dispatch is correctly stamped on every path. Opaque state is refused in both directions
and ingress constructs none. No diagnostic interpolates a request or response byte. The scenario
and report arithmetic is consistent, and pass 1's coverage-disclosure finding is corrected.

The pass named one deviation of its own rather than hiding it: it took its session lease after
probing rather than before, and released only its own.
