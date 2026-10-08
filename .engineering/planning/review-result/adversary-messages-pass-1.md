---
format: aep.planning-md/3
id: review-result:adversary-messages-pass-1
kind: review-result
status: archived
title: 'Adversary, messages projection, pass 1: needs-change on three blockers'
relations:
- reviews: story:messages-projection
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:35Z", actor: "human:timo", revision: 2}
---
## The pass

First adversarial pass over `story:messages-projection`, wave 1, against `wave1-messages` over
base `dbc45f8`. The unit had reported green: 40 tests, 35 conformance scenarios passing three
times, 23 mutations each killing a named scenario.

Verdict: **needs-change**. Seven cases added, six red. Eight findings: four introduced, four
carried in with the draft this unit inherited. All 40 of the unit's own cases stay green, and the
conformance suite stays at 35 passing while six cases are red — which is the result, not an
aside: none of the six defects is visible to that lane.

## Two blockers are acceptance the contract names and nothing covers

The story's implementation contract says caller cancellation **and the absolute turn deadline**
cover secret resolution, the HTTP exchange and blocked sinks. The documentation restates it.

Neither deadline half exists. Credential preparation is awaited before the HTTP client is entered,
and it selects on the cancellation token alone. The stream decoder awaits the sink between two
reads of the transport, under a select whose only other branch is cancellation. Measured both
ways: a 300 millisecond total limit, a resolver that never answers and a sink that never accepts,
both still running after three seconds with nothing cancelled.

The unit's own suite tests the cancellation half of both and neither deadline half. A credential
store that stops answering holds a turn open with no bound, and this workspace already ships three
resolvers and models a non-answering one in another crate's tests, so the shape is known here.

## The laundering defect, in a second projection and frozen into a contract

A sibling projection was found stamping the reading binding onto state it could not attribute.
This unit has the same defect, and it arrived with the carried draft rather than being written
this session.

What makes it worse here is what the unit did next. Its own scenario asserts the laundered
provenance **as correct** — the opaque item's binding recorded as the reader's. So the defect is
now written into a contract document, where it reads as intended behaviour. The crate's own
documentation says opaque state from any other binding is refused rather than replayed, and the
neutral model's own comment says payloads stay bound to their original serving model.

The pass also found why no scenario could see it: the conformance adapter renders an opaque item
as its payload alone and discards the provenance, so the only fact that would expose it is the one
the single scenario asserts the wrong value for.

## The class that beat two siblings did not beat this one

Each component of the inclusive input, dropped one at a time, leaves the total unknown and
preserves the others. The pass wrote that case and it passed. Cumulative usage assigns rather than
accumulates, and its fixture would show a sum if one happened. A rejected report leaves the
previous snapshot intact. This unit is the only one of the three projections where that class was
genuinely closed.

The pass also re-derived every refusal in the ordering scenarios and confirmed the unit had found
all three that were passing for an unnamed reason, not merely the ones it reported.

## The carried draft has three more unverified names, and one is documented as confirmed

The unit flagged one field it could not re-read against the producer's types. Three more arrived in
the same draft with exactly that status and are not flagged, and the crate's documentation states
one of them positively as accepted and unused, which reads as confirmed and is not.

## Bound on what the pass touched

No tracked file was modified: the unit's diff is byte-identical before and after. Everything added
is three untracked test files. The two origin questions were settled by reading the base revision
out of Git rather than by moving the tree.

## Report

```
unit: story:messages-projection
verdict: NEEDS-CHANGE
cases: executed 40→47, red 6
origin: introduced 4 / pre-existing 2 / undecided 0
wrote-outside-worktree: 1 path
needs-coordinator: none
```

```findings
- file: crates/llm-messages/src/codec.rs
  line: 390
  category: contract-drift
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: "ingress stamps the reading binding onto a thinking block it cannot attribute, so opaque state the outgoing path refuses is sendable after one round trip, and the unit's own ingress scenario asserts the laundered provenance as correct."
- file: crates/llm-messages/src/client.rs
  line: 62
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the absolute turn deadline does not cover credential resolution, which is awaited before the HTTP client is entered and selects on cancellation alone, so a secret store that stops answering holds the turn open without bound despite the contract naming secret resolution."
- file: crates/llm-messages/src/decode.rs
  line: 562
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the absolute turn deadline does not cover a sink that never accepts, because the sink is awaited between two transport reads under a select whose only other branch is cancellation, so a blocked caller hangs the turn indefinitely despite the contract naming blocked sinks."
- file: crates/llm-messages/src/usage.rs
  line: 25
  category: judgement
  severity: warning
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: "three more producer wire names arrived unchanged with the carried draft and have exactly the unverified status the unit flagged for one field, yet only that one is named as unestablished while the documentation states another positively as accepted and unused."
- file: crates/llm-messages/src/decode.rs
  line: 413
  category: concurrency
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "assembled items are appended in block-stop order rather than index order, so two open blocks stopped out of order reverse the turn relative to the deltas the caller was streamed; the pass constructed the stream and no documented producer emits it."
- file: crates/llm-messages/src/codec.rs
  line: 173
  category: boundary
  severity: warning
  verdict: INFEASIBLE
  origin: pre-existing
  message: "seven fields on the ingress accept list are refused when spelled as the explicit null that means absent, while the response half of the same codec reads null as absent in five places; no ingress caller was demonstrated."
- file: crates/llm-messages/src/decode.rs
  line: 270
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the stream's message start never checks the inner message's type or role although the non-streaming reader does and both share one field list, so a stream announcing an error type and a user role decodes into a completed assistant turn."
- file: docs/verification/messages.md
  line: 68
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the falsification section says two scenarios were rewritten because a mutation did not kill them, and then names three."
```

## What the pass attacked and could not break

Cumulative usage replaces rather than sums, and the fixture would show a sum. A rejected report
rolls back to the previous snapshot. Regression is caught per counter and overflow is refused.
One-hour cache creation and server-tool use are refused rather than approximated. Signed and
redacted thinking survive, as do tool-result failure flags. A diagnostic carries no upstream text:
the pass pinned that against a payload containing an internal trace identifier and a tenant name.
Every counted claim in the verification record — forty tests, twenty-six authored scenarios,
thirty-five selected — is true.
