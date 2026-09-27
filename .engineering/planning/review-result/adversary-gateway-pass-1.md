---
format: aep.planning-md/2
id: review-result:adversary-gateway-pass-1
kind: review-result
status: active
title: 'Adversary, gateway authentication, pass 1: three blockers, one refuting the coordinator''s record'
relations:
- reviews: story:gateway-auth
revision: 1
---
## The pass

First adversarial pass over `story:gateway-auth`, wave 1, against `wave1-gateway` over base
`f63386e`, where every attacked file is absent. The unit had reported green: 23 cases, 11
mutations each killed by a named case, three identical runs.

Verdict: **confirmed, three blockers**. Six cases added, all six red. Eleven findings, all
`introduced`. The pass changed no implementation file; its only write to the worktree is one test
file.

## The blocker that refutes the coordinator's own record

The unit declared zero dependencies and asserted, in a test, that nothing able to resolve a secret
or provision a resource is linked. The coordinator recorded that this made the acceptance
criterion a fact about the dependency graph rather than a discipline somebody maintains.

**As implemented, it is neither.** The check splits the metadata output at the first `]`, which
closes the *first dependency's own features array*. Measured against a real workspace package, it
examined **1 of 18** declared dependencies and missed two forbidden crates. The other half runs
metadata with dependencies suppressed, which emits no resolve section at all, so no transitive
edge is examined: the crate the documentation names as the natural next dependency reaches a
credential crate and an async runtime in two hops, and neither is on the ten-name list.

The guard is vacuously correct today only because the crate declares nothing, and it is wholly
prospective — it exists to catch an edit that has not happened. A prospective guard that does not
work is the worst kind, because it is trusted and never exercised. The unit's own verification
record does disclose the suppressed-dependency limitation; its user-facing document contradicts
that disclosure.

## Two blockers a deployment would meet on its first bad day

The liveness probe is documented as always answering success while the process serves, and as
matched before anything else. The concurrency guard runs before the path match, so under load the
liveness probe answers unavailable. A liveness probe that fails under load restarts the process,
which converts load-shedding into a restart loop.

Shutdown is documented as letting every accepted connection finish. It clears readiness first, so
a connection already accepted is refused — while the shutdown report counts that same connection
both as in flight at the signal and as completed. The documented invariant that completed equals
accepted therefore holds *over a refusal*, which is worse than the invariant failing, because
nothing signals it.

## The fixture shape that has now beaten four units

The documentation argues there is no field for an endpoint URL or a secret reference, therefore no
inspection response can carry one. The premise is true and the conclusion does not follow: the
label type accepts any printable ASCII, so a URL or an API key fits. The unit's own leak test
passes because its fixture chose opaque names.

That is the same shape found in three sibling units: every fixture is the complete, in-subset
case, so the guard that would fire outside it is unreachable. Three further findings are the same
class at a smaller scale — a minimum-length constant that can be lowered from 32 to 8 with the
suite green because the only case uses 5 and 43 bytes; two size bounds with no case, whose error
variants are the only ones in the crate nothing provokes; and a published status table whose
column is compared to nothing, so six of ten statuses are asserted only against themselves.

## Report

```
unit: story:gateway-auth
verdict: CONFIRMED (3 blockers)
cases: executed 23→29, red 6
origin: introduced 11 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths
needs-coordinator: none
```

```findings
- file: docs/gateway.md
  line: 72
  category: contract-drift
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the concurrency guard runs before the probe path match, so the liveness endpoint answers unavailable under load despite the contract promising it always succeeds while the process serves, which turns load-shedding into a restart loop."
- file: docs/gateway.md
  line: 118
  category: concurrency
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "shutdown clears readiness before draining, so a connection already accepted and counted as in flight at the signal is refused, while the shutdown report still counts it completed and the documented completed-equals-accepted invariant holds over that refusal."
- file: crates/llm-gateway/tests/dependency_boundary.rs
  line: 57
  category: mutant
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "the metadata scan splits at the first bracket, which closes the first dependency's own features array, so it examined one of eighteen declared dependencies on a real workspace package and missed two forbidden crates."
- file: docs/gateway.md
  line: 13
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the boundary is called a property of the dependency graph, but metadata with dependencies suppressed examines no transitive edge, and the crate the document names as the natural next dependency reaches a credential crate and an async runtime in two hops."
- file: docs/gateway.md
  line: 93
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the published refusal table's status column is never compared to the code, and six of ten statuses are asserted only against themselves, so a status mutation ships a stale contract green."
- file: crates/llm-gateway/src/auth.rs
  line: 15
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the minimum shared-secret length can be lowered from 32 to 8 with the suite green, because the only case uses a 5-byte and a 43-byte secret, leaving the documented floor unguarded."
- file: docs/gateway.md
  line: 56
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "the claim that no inspection response can carry an endpoint URL or secret reference is a non sequitur, because the label type accepts any printable ASCII; the unit's own leak test passes only because its fixture chose opaque names, and no adapter populates these fields yet."
- file: crates/llm-gateway/tests/gateway.rs
  line: 520
  category: mutant
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "the four-way refusal closure reduces to one length equality over a line-based parse of the error source, defeated by a variant declared after a brace inside a doc comment, but only by a deliberately shaped edit."
- file: crates/llm-gateway/src/error.rs
  line: 81
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the unavailable diagnostic says the gateway is not ready to serve inspection, which misstates the cause on the overload path where it is ready."
- file: crates/llm-gateway/src/inventory.rs
  line: 18
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the route and target size bounds have no case, making their two error variants the only ones in the crate nothing provokes, in a crate that gave every refusal code an exhaustive provocation test."
- file: crates/llm-gateway/src/server.rs
  line: 127
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "marking readiness silently clears the draining flag, so a periodic re-mark in an embedding returns a draining gateway to rotation, which no document mentions."
```

## What the pass attacked and could not break

No authentication bypass by route, method, header casing, duplication, obsolete folding or
malformed head. No refusal, header or body carries any caller byte, and the crate's source
contains no logging or printing macro at all. The credential comparison folds the length
difference and every overlapping byte. The head-bound arithmetic, terminator search and saturation
are correct, and the concurrency bound admits exactly its maximum. The shutdown report's join
logic is sound. Unknown stays unknown across the whole inventory surface: the optional fields are
omitted rather than defaulted, and the enumerations are variant-for-variant identical to the
neutral ones, so no lossy default is forced on whatever composes this later.
