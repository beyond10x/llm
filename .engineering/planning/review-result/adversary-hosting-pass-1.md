---
format: aep.planning-md/1
id: review-result:adversary-hosting-pass-1
kind: review-result
status: active
title: 'Adversary, hosting contract, pass 1: two blockers, and five of six new mutations survived'
relations:
- reviews: story:hosting-contract
revision: 1
---
## The pass

First adversarial pass over `story:hosting-contract`, wave 1, against `wave1-hosting` over base
`f63386e`. The unit had reported the strongest evidence in the wave: 50 Rust cases, 49 scenarios
passing three times, and **21 deliberate defects of which 21 were killed and none survived**.

Verdict: **needs-change**. Seven cases added, seven red. Twelve findings, all `introduced`.

## The headline claim did not survive

Six new mutations, each applied to a copy outside the worktree and restored with the hash
verified. **Five of the six survived the unit's entire suite** — all 50 Rust cases and all 49
scenarios green under each:

| mutation | result |
|---|---|
| the resource ceiling multiplied by ten | survives |
| the resource-ceiling guard deleted outright | survives |
| the time-ceiling comparison loosened at its boundary instant | survives |
| the lifetime-ceiling comparison loosened at its boundary | survives |
| the effective-lifetime calculation inverted from minimum to maximum | survives |
| the identifier-length comparison loosened | killed |

Every fixture uses the same ceiling value and the same lifetime pair, so no fixture is ever at a
boundary. This is the same defect that beat the four units before it — fixtures that are all the
ordinary, in-range case cannot reach a guard that fires at the edge — and it beat the unit that
had been warned about it explicitly and had answered one instance of it.

Twenty-one kills said nothing about the guards no mutation was aimed at. That is the whole lesson
of this wave in one line, and the unit with the best record is where it is clearest.

## Two blockers, both about money

**A foreign owner tag that is not strictly newer is silently ignored.** The branch written for
that case is guarded on a transition to uncertainty, and neither of the two phases that own a live
resource has an edge to it, so the branch can never fire from either. The record stays active with
no obligation, and the next stop destroys the other owner's compute. The code comment beside it
and the documentation both say the opposite happens.

**A complete listing that echoes no request identifier discharges an open ambiguous create.** The
obligation settles as stopped, with evidence naming the absence, while the resource is listed and
running. Completeness means every resource was listed; it is not a promise that every listed
resource echoes an idempotency key. An unknown outcome keeping its obligation open is the single
requirement this story exists for, and here it is inverted into a false stop.

## Two published refusals nothing can emit

The macro that generates the closed vocabularies guarantees an inventory cannot drift from its
variants. Two of those variants have zero constructors anywhere in the crate: the refusal for a
resource that is not the one owned here, and the stop reason for a restarted owner recovering a
record. Both are published, and one is published twice, in the domain document as well.

The second finding about that macro is the sharper one. It prevents drift **inside Rust**, and
that is where the guarantee stops. The same four vocabularies are re-declared by hand in the
domain document, nothing compares the two, and the observed fields are typed as free strings
rather than the declared enumerations — so the executable specification never checks an observed
value against the variants either.

## Report

```
unit: story:hosting-contract
verdict: NEEDS-CHANGE
cases: executed 99→106, red 7 (Rust 50→56 red 6; scenarios 49→50 red 1)
origin: introduced 12 / pre-existing 0 / undecided 0
wrote-outside-worktree: 3 paths
needs-coordinator: none
```

```findings
- file: crates/llm-provision/src/controller.rs
  line: 707
  category: acceptance
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "a foreign owner tag without a strictly newer epoch is a silent no-op for the two phases that own a live resource, because neither has an edge to the uncertain phase the branch is guarded on, so the record stays active with no obligation and the next stop destroys the other owner's resource."
- file: crates/llm-provision/src/controller.rs
  line: 643
  category: boundary
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "a complete listing whose resources carry no request identifier discharges an open ambiguous-create obligation as stopped while the resource is listed and running, inverting the one requirement this story exists for."
- file: crates/llm-provision/src/spec.rs
  line: 28
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the resource ceiling can be multiplied by ten or its guard deleted outright with all 50 Rust cases and all 49 scenarios green, because every fixture uses the same small value."
- file: spec/domains/hosting.yaml
  line: 13
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the macro prevents vocabulary drift only inside Rust; the same four vocabularies are re-declared by hand in the domain document, nothing compares them, and the observed fields are typed as free strings so the specification never checks a value against the declared variants."
- file: crates/llm-provision/src/error.rs
  line: 36
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the refusal for a resource that is not the one owned here is published in the generated inventory and has zero constructors anywhere in the crate."
- file: crates/llm-provision/src/machine.rs
  line: 119
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the stop reason for a restarted owner recovering a record has zero constructors and is published both in the Rust inventory and in the domain document."
- file: crates/llm-provision/src/machine.rs
  line: 91
  category: judgement
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "an open stop obligation leaves the outstanding totals when the record becomes disowned, with no stop evidence, and nothing in this contract picks it up despite the code saying the obligation moved with the ownership."
- file: crates/llm-provision/src/spec.rs
  line: 91
  category: judgement
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the public effective-lifetime function has zero callers while the controller open-codes the same expression, so inverting it from minimum to maximum survives the whole suite."
- file: crates/llm-provision/src/spec.rs
  line: 83
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "a requested lifetime exactly equal to the policy ceiling, which the documentation allows, has no case, so loosening the comparison survives the suite."
- file: crates/llm-provision/src/controller.rs
  line: 293
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the time ceiling's boundary instant has no case; the existing case ticks past the deadline only, so loosening the comparison survives the suite."
- file: crates/llm-provision/src/controller.rs
  line: 662
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "the create settlement adopts whatever resource the answer carries without checking it names this provider, account or owner, and a later stop is addressed to it; only a non-conforming provider reaches this, and the pass built the one that does."
- file: docs/hosting.md
  line: 48
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "exactly two things close an obligation is immediately followed by a four-row table."
```

## What the pass attacked and could not break

The transition table is asserted whole in two independent places, every phase assignment goes
through the guard, and the only ungated write is the initial one — so the enforced table cannot
exceed the published one. Name-reuse fencing holds: adoption matches the whole resource key
including the incarnation, and the pass could not construct an adoption of a reused name. The
requested and observed shapes share no constructor and no field falls back to the request. Lease
epoch monotonicity across release and restart, inclusive expiry, and the one-of-many concurrent
grant all hold with boundary cases. Disconnect, release and lease expiry each have a case proving
they are not shutdown, and each survived.

One thing the pass read and deliberately did not raise: the restart path takes an instant with no
lower bound and the clock-reversal guard is instance-local. It says plainly that it did not run
that and is not claiming it.
