---
format: aep.planning-md/2
id: review-result:adversary-hosting-pass-2
kind: review-result
status: active
title: 'Adversary, hosting contract, pass 2: the ownership defect survived its fix through the reason slot'
relations:
- reviews: story:hosting-contract
revision: 1
---
## The pass

Second and final adversarial pass over `story:hosting-contract`, wave 1, against `wave1-hosting`
over base `f63386e`. The attack budget is now spent.

Verdict: **needs-change**. Five cases added, all five red. Eleven findings, all `origin: introduced`;
one blocker, six warnings, four notes.

Executed cases 146 → 151. The before figure — 75 Rust plus 7 conformance plus 64 scenarios — was
measured on a scratch copy with the new files deselected, and agrees with the unit's own
`docs/verification/hosting.md`.

What the pass touched is proved by a recursive compare of the worktree against a pristine copy taken
before the first probe: two files differ, `crates/llm-provision/tests/adversary_pass2.rs` and
`contracts/hosting/scenarios/adversary-a-requested-stop-must-not-hide-a-foreign-owner-tag.yaml`, both
tests. No implementation file differs, which is also the proof that every mutation applied to the copy
was restored byte for byte.

## The blocker: the pass-1 defect survived its own fix, through a second slot

Pass 1 found that a foreign owner tag which is not strictly newer was silently ignored, and that the
next stop would then destroy another owner's compute. The correction answered the branch it was
pointed at. The defect is still reachable through the reason slot.

`Stop`'s foreign-resource refusal at `crates/llm-provision/src/controller.rs:604` is keyed on
`record.stop_reason == Some(StopReason::OwnershipLost)`. `require_stop` at `:845` is documented as
never overwriting the reason that opened an obligation. So when an obligation is already open,
`adopt`'s foreign-owner branch at `:793` records nothing, the refusal never fires, and the stop
mutation is addressed to the resource the provider says belongs to `controller-b`.

Measured: `tests/adversary_pass2.rs:160`, `:225` and `:275` — `fake.stops()` is 1 where 0 is required,
and `controller-b`'s resource is gone from the provider's own inventory. The conformance lane carries
the same defect through the unit's adapter: `stops = 1.0`, `stop_reason = "operator-request"`,
`phase = "stopped"`, `stop_required = []`. The controller destroyed another owner's compute and filed
it as its own discharged obligation. Whole-crate exit 101; conformance gate exit 1,
`{"total":65,"passed":64,"failed":1,"error":0,"unsupported":0,"skipped":0}`.

**What reaches it** — this is not a fixture construction. Two ordinary sequences: `RequestStop`,
`Observe`, `Stop`, which is the documented operator flow; and lease expiry, reacquisition by another
controller, `Observe`, `Stop`. Every case the unit wrote for a disputed resource reaches the retag from
`Active` with `stop_reason: None`, which is why none of them sees this.

The two sites that record one fact disagree about precedence: `settle_create:735` already sets
`OwnershipLost` unconditionally for the same fact, and `require_stop` refuses to. The named fix, not
applied by the adversary: make ownership-lost win at both, or key `Stop`'s guard on the last
observation rather than on the reason slot.

## The six claimed corrections, checked by running them

| Claim | Result |
|---|---|
| all five pass-1 mutation survivors are now killed | holds — each re-applied to the copy, each killed by a named case in both lanes, each restored with SHA-256 equal before and after |
| fourteen bounds measured at the literal and one past it | holds — every expected value in `hosting.rs:980-1167` and `:703-758` is a literal, not `CONSTANT ± 1`; no fifteenth bound found |
| a flipped row in the phase-change table fails a test | incomplete — flipping any listed row does fail, but the table omits `Declared -> StopRequired` |
| every published refusal and stop reason produced through real public calls under an exhaustive match | holds — no variant reachable only from test code |
| the Rust-versus-specification vocabulary comparison is a test | holds — it reads `spec/domains/hosting.yaml` through `include_str!` and the unit's own defect kills it |
| the deleted scope guard's state is unconstructable | the state is unconstructable; the argument for deleting the guard is not itself checked |

Two further coordinator claims were checked. The case the coordinator rewrote does not weaken the
decision, but its second assertion is vacuously true. The bound the unit said it could not close is
stated more narrowly in the documentation than the code behaves.

Scenario selection was checked against the class found on `story:chat-projection` and does not
reproduce here: 57 files select 64 and execute 64; 58 files select 65 and execute 65.

## Six guards that no mutation was ever aimed at

The wave's recurring class again, in its weaker form: not fixtures that cannot reach a guard, but
guards nothing was aimed at. Each of these survives deletion with all 75 Rust cases and all 64
scenarios green.

| site | what deleting it allows |
|---|---|
| `controller.rs:845` | last-writer-wins on the stop reason — the precedence rule the blocker turns on, asserted in neither direction |
| `controller.rs:478` | a second `Provision` on a record that already submitted one, which against a provider without idempotency is a second billed resource for one authorization |
| `controller.rs:464` | a mutation by a controller whose own claim expired with nobody taking over |
| `controller.rs:189` | a restored record whose *requested* provider or account belongs to another scope |
| `controller.rs:773` | `adopt` taking a key from outside its scope — one of the three legs cited at `:615-619` to justify deleting `Stop`'s scope check |
| `controller.rs:726` | `settle_create` accepting a created resource tagged with another controller's label, which no fixture produces |

## What could not be broken

Name-reuse fencing across epochs, lease-epoch monotonicity, inclusive expiry, the one-of-many
concurrent grant, `requested` versus `observed` non-leakage, `Unknown` never treated as a no-op, and
`answers_request_identity`. `Controller::restore` with duplicate or mismatched deployment keys is
unreachable from `snapshot()` and is not raised as a finding.

## For the coordinator

`home-path:sha256:e8b3de0953ea03ac3b5d346f9a83b92568863ee24fdef2a4ee94696b3a0dedd2` adds 57 `hosting/scenarios/…` lines and needs
a 58th for the scenario this pass added, or that scenario is never selected by the combined suite.
`contracts/ess-inputs.yaml` is the coordinator's file and the adversary did not edit the patch.

```findings
- file: crates/llm-provision/src/controller.rs
  line: 604
  category: acceptance
  severity: blocker
  verdict: CONFIRMED
  origin: introduced
  message: "Stop's foreign-resource refusal is keyed on stop_reason == OwnershipLost while require_stop never overwrites an existing reason, so a resource the provider says belongs to controller-b is stopped whenever an operator request or a lease expiry filled the reason slot first, and the record files the destruction as its own discharged obligation."
- file: crates/llm-provision/src/controller.rs
  line: 845
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "deleting the is_none() wrapper that makes require_stop first-writer-wins leaves all 75 Rust cases and all 64 scenarios green, so the precedence rule the blocker turns on is asserted in neither direction."
- file: crates/llm-provision/src/controller.rs
  line: 478
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "deleting provision's phase guard survives the whole suite, and that guard is the only barrier to a second Provision submitting a second billed resource for one authorization against a provider that does not honour idempotency keys."
- file: crates/llm-provision/src/controller.rs
  line: 464
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "deleting fence's live_at disjunct survives the whole suite; it is the only branch refusing a controller whose own claim expired with nobody taking over, and the expired-lease case asserts the obligation rather than the refusal."
- file: crates/llm-provision/src/controller.rs
  line: 189
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "deleting restore's InvalidSpec check on a restored record's requested provider and account survives the whole suite; the one restore-scope case mutates observed.key.account only."
- file: crates/llm-provision/src/controller.rs
  line: 773
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "deleting adopt's key_in_scope guard survives the whole suite, and that guard is one of the three paths cited at controller.rs:615-619 to justify deleting the scope check in Stop, so the deletion rests on a leg nothing measures."
- file: crates/llm-provision/src/controller.rs
  line: 726
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "neutralising the owner_is_ours disjunct in settle_create survives the whole suite, because FakeProvider always tags a created resource with the requesting owner and no fixture answers in scope with another controller's label."
- file: crates/llm-provision/src/machine.rs
  line: 134
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the INTENTS table claims to hold every phase change the controller asks for, but omits Declared -> StopRequired, which request_stop asks for and which is still a silent no-op reported as Ok with no obligation recorded, where Cancel refuses the same class with wrong-phase."
- file: crates/llm-provision/src/controller.rs
  line: 315
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "three guards that cannot be tripped remain written despite the unit's own rule against them: elapse's terminal skip, discharge's may_become(Stopped) early return at :852, and fence's owner disjunct at :462, all of which survive deletion and none of which can fire."
- file: docs/hosting.md
  line: 148
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "the closing bound is stated as 'a snapshot holding no records witnesses nothing', but witnessed() returns 0 for any Declared or Cancelled record too, so a non-empty snapshot also reopens at an arbitrarily earlier instant; no harm could be constructed, since such records carry no started instant and restore drops the lease."
- file: crates/llm-provision/tests/adversary_ownership.rs
  line: 384
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the coordinator's rewrite asserts the decision correctly but its second assertion, that every stopped key names account-1, is vacuously true because Stop is never applied in that case."
```
