---
format: aep.planning-md/1
id: review-result:adversary-gateway-pass-2
kind: review-result
status: active
title: 'Adversary, gateway authentication, pass 2: retirement audited sound, two findings are its residue'
relations:
- reviews: story:gateway-auth
revision: 1
---
## The pass

Second and final adversarial pass over `story:gateway-auth`, wave 1. The attack budget is spent.

Verdict: **needs-change**. Seven cases added, four red. Seven findings: five introduced by the
unit, **two `undecided` — residue of the coordinator's own retirement decision, not the unit's
code**. No implementation file was modified; the pass created one test file and nothing else.

## It audited the coordinator's retirement, which is what it was asked to do

Pass 1 left six cases. The coordinator retired four of them, having verified that each asserted a
property of a mechanism the correction had replaced. This pass was told to check that decision and
to report a finding against the coordinator if any retirement lost coverage.

**None did**, and the pass established it by blinding each replacement rather than reading it:

| Retired case | Replacement | Checked how |
|---|---|---|
| the declared-dependency scan reading past the first dependency | the whole-array read | blinded the scan to split at the first bracket: 1 of 18 dependencies seen, neither forbidden crate found, the control's assertion fails |
| the transitive closure being examined at all | the closure walk | the named crate's closure really holds 72 packages including both forbidden ones; blinded to the root it yields one package and the control fails |
| a refusal code reaching a client undeclared | the generating macro | no edit adds a variant outside it |
| an inspection response carrying an endpoint URL | the byte-rendering case | the retired case asserted something false, so nothing true was lost |

Both dependency controls are therefore genuine rather than decorative, which is the property that
made the retirement safe.

## What the retirement did leave behind, and it is the coordinator's

The unit's verification record still describes those four cases as shipped and red, and declares
the adversary lane failed with 39 cases against the 35 the sources now declare. A unit whose own
record says its suite failed cannot be read as merging clean. The cause is the retirement, not the
unit's work, which is why both findings carry `undecided` rather than `introduced`.

## The five that are the unit's

The sharpest is a third instance of a shape pass 1 named twice in this same crate: a true premise
with a false conclusion. The authentication proof "cannot be constructed except from the verdict,
**so** no inspection entry point can be reached before authentication" — but the verdict's owner
variant is a public unit variant, so any caller mints the proof, and the crate's own tests do. A
second path reaches the same identifier bytes with no proof at all.

Two are checks that do not check what they claim. The bounds table says every number in it is
verified against measured behaviour; for two of its seven rows the case reads a configuration
default instead, so a change to the enforcement ships green. And the table claims to list every
bound the crate enforces while omitting one, which its own completeness check cannot see because
it compares the table to a second hand-written list rather than to the source.

## Report

```
unit: story:gateway-auth
verdict: NEEDS-CHANGE
cases: executed 35→42, red 4
origin: introduced 5 / pre-existing 0 / undecided 2
wrote-outside-worktree: 2 paths
needs-coordinator: two findings are residue of the coordinator's retirement
```

```findings
- file: docs/verification/gateway.md
  line: 16
  category: contract-drift
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: undecided
  message: "the verification record names four cases as shipped and red and declares the adversary lane failed, and none of the four exists in the tree after the coordinator's retirement."
- file: docs/verification/gateway.md
  line: 9
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: undecided
  message: "the record states the suite runs 39 cases against the 35 its six named sources declare."
- file: docs/gateway.md
  line: 161
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the bounds table claims to list every bound the crate enforces and omits the digest-length bound, which the published check cannot detect because it compares the table to a second hand-written list in the test file rather than to the source."
- file: crates/llm-gateway/tests/gateway.rs
  line: 759
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "two of the seven published bounds are asserted by reading configuration defaults rather than by measuring enforcement, so the claim that every published bound is checked against measured behaviour is false for those rows."
- file: crates/llm-gateway/src/auth.rs
  line: 96
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the owner verdict is a public unit variant so any caller mints an authentication proof without verifying anything, and a second path exposes the same identifier bytes with no proof at all, making the documented conclusion a non sequitur for the third time in this crate."
- file: crates/llm-gateway/src/server.rs
  line: 497
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the reason phrase is the one property of a refusal the generating macro does not produce and nothing compares to the code's status, so a variant with an unlisted status ships a status line reading unknown with the whole suite green."
- file: docs/gateway.md
  line: 154
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the shutdown invariant written to close the first pass's blocker states without qualification that a connection counted as completed was not answered with a refusal, and the report counts every ordinary refusal as completed."
```

## What the pass attacked and could not break

Liveness under load holds at zero and one concurrent request, with a half-open connection held and
with sixty-three slots occupied. The shutdown ordering is correct and the pass could construct no
shutdown-caused refusal: a connection accepted inside the poll window is served and joined, and
accepted equals completed after the join. Both bounds it checked behaviourally are enforced at the
published number and one past it. Every universal quantifier in the crate is guarded by a
preceding emptiness or length check. No caller byte appears in any refusal head or body, the
challenge header is present on every unauthenticated refusal, and the published table is checked
in both directions including its status column.
