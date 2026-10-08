---
format: aep.planning-md/3
id: review-result:adversary-responses-pass-2
kind: review-result
status: archived
title: 'Adversary, responses projection, pass 2: the first correction was unreachable by fixture'
relations:
- reviews: story:responses-projection
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:36Z", actor: "human:timo", revision: 2}
---
## The pass

Second and final adversarial pass over `story:responses-projection`, wave 1. The attack budget is
now spent.

Verdict: **needs-change**. Five cases added, all five red. Seven findings, all `introduced`. The
pass ran no mutation at all: every finding is driven by a fixture, and it declined to run the
unit's own falsification harness because that harness edits the two implementation files in place.
Both implementation digests still match the unit's own recorded values.

## The correction that looked done

Pass 1's first blocker was that a refusal raised while decoding the terminal object's own output
returned with no observation, losing the usage that object had just reported. The unit answered it
by enumerating all seven error sites and attaching the evidence at each.

The enumeration was right about the function it enumerated. The defect lives outside it. A real
server announces a finished item with its own event before the terminal object arrives; the
decoder handles it there, refuses through a different path, and stops — so the terminal object in
the **same payload list** is never read and its counters are lost exactly as before.

**No fixture anywhere in the tree sends that event.** Not the 46 Rust cases, not the 50 authored
scenarios, not the generated suite. The one case that appears to cover it sends the event name with
no item attached, so the branch body is never entered. The correction is reachable only from a
payload shape no server produces, which is why it looked reached.

This is the same defect surviving its own fix, not a new one. It is the reason the budget is spent
rather than extended.

## Two vacuous-truth defects the crate documents and defends against elsewhere

A body that **omits** a fixed field is accepted and answered with this crate's own value, because
the guard reads absence as agreement. A non-streaming request, which is the default form on this
wire, comes back as a streaming one; a stored conversation comes back unstored. The documentation
says all four fixed fields are refused when they carry a different value, and absence carries a
different value — the crate argues exactly that itself, about a different field, a hundred lines
away.

An explicitly empty include list passes its guard because a universal over no elements is true,
and is then answered with a request for encrypted reasoning content the client never asked for.
The crate guards against precisely this trap in another function, with a comment explaining it.

## The harness that was supposed to make the record honest

Pass 1 found that a falsification record can claim more coverage than it has. The unit answered by
making the harness fail outright when a mutation is killed by neither lane. This pass read the
harness rather than running it, and that claim does not hold: a mutation that fails to compile is
recorded as defended and never reaches the undefended check; any Rust failure counts as a kill
rather than the named one; and the conformance lane ignores the run's exit status and re-reads a
report file with no freshness check, so a run that produced no report yields the previous
mutation's failures.

Separately, the harness runs the Rust lane without disabling fail-fast, so it only ever observes
the first failing target. Two rows of the shipped record name a case the machine record does not
list as having failed. The pass demonstrated the mechanism on its own run: with an earlier target
failing, 35 cases never executed at all.

The shipped record's 23 rows are all killed by both lanes, so the record is not itself corrupted.
The claim about the harness is what is false.

## Report

```
unit: story:responses-projection
verdict: NEEDS-CHANGE
cases: executed 46→51, red 5
origin: introduced 7 / pre-existing 0 / undecided 0
wrote-outside-worktree: none
needs-coordinator: whether two findings are answered in code or in the document is the implementor's call
```

```findings
- file: crates/llm-responses/src/stream.rs
  line: 148
  category: acceptance
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "a tool call refused at the item-done event returns through a path that does not retain evidence and stops the decode, so the terminal object in the same payload list is never read and its counters are lost; no fixture in the tree sends that event, which is the only reason the first correction looks reached."
- file: crates/llm-responses/src/request.rs
  line: 129
  category: boundary
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the guard reads an omitted fixed field as agreement, so a non-streaming stored request is accepted and answered with a streaming unstored one, against the refusal the documentation promises for all four fixed fields."
- file: crates/llm-responses/src/request.rs
  line: 145
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "an explicitly empty include list passes its guard vacuously because a universal over no elements is true, and is then answered with a request for encrypted reasoning content the client did not ask for."
- file: crates/llm-responses/src/request.rs
  line: 490
  category: property
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "two text parts in one in-subset message are merged into one on reprojection, so the stated property that anything ingress accepts is sent back unchanged is false, and the adapter's preservation fact cannot see it because it compares only fields the body sent."
- file: docs/verification/responses.md
  line: 60
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the claim that the harness fails outright when a mutation is killed by neither lane is false for a mutation that does not compile, which is recorded as defended; it also counts any Rust failure as a kill rather than the named one, and reads the conformance report without checking the run's exit status or freshness."
- file: docs/verification/responses.md
  line: 78
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "two falsification rows name a case the machine record does not list as failed, because the harness runs the Rust lane without disabling fail-fast and only ever observes the first failing target."
- file: crates/llm-responses/src/stream.rs
  line: 294
  category: contract-drift
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "the provider-failure path discards its own classification when the counters disagree, so a rate-limited failure is reported as non-retriable and carries no evidence, against two documented sentences; no provider was shown that both fails a response and reports contradictory counters."
```

## What the pass verified and could not break

Ingress mints no provenance anywhere: the crate constructs no opaque item on that path at all, and
the entry match is exhaustive with a refusal default. Pass 1's laundering finding is genuinely
closed. The seven-site enumeration is correct about the function it covers, and the one refusal
that deliberately carries no evidence is the only one in it. The rewritten test asserts something
real rather than merely something true — only one guard can produce the refusal it names, so it
cannot pass vacuously. The refusal-table cases fire the right refusal: the pass checked all
eighteen ingress fixtures for a second guard that could fire first with the same code, and found
none. The counter-independence rule, the never-substituted model, upstream text never reaching a
diagnostic, and the tool-name class on both sides all hold.
