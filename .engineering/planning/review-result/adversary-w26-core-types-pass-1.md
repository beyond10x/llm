---
format: aep.planning-md/3
id: review-result:adversary-w26-core-types-pass-1
kind: review-result
status: active
title: Wave 2026-11-05-w26 adversary pass, llm story:core-types-non-exhaustive, pass 1
relations:
- reviews: story:core-types-non-exhaustive
revision: 1
---
```
unit: beyond10x/llm story:core-types-non-exhaustive, commit 372ddc5e6 (wave merge 19af9c41e)
verdict: NEEDS-CHANGE
cases: executed 91→95, red 1
origin: introduced 4 / pre-existing 2 / undecided 0
```

Nothing wrong reached a wire. Two of the unit's own statements were false: `docs/design.md` said a
match inside llm-core names every variant, while `item.rs:158` and `turn.rs:149`, `:235`, `:412`
ended in a catch-all; the CHANGELOG Breaking lines did not name the consumer cost. A setter swap of
`temperature` and `top_p` survived the unit's setter test. Fixed in 436a087c8 (merged 688fe84ee): the
four matches name their variants, the CHANGELOG names harness's wildcard arm and control-plane
staying on 0.4.0, and the cases in `crates/llm-core/tests/adversary_w26_non_exhaustive.rs` are
committed. `docs/harness-parity.md:306` is a historical record and stays.

```findings
- file: docs/design.md
  line: 27
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: four llm-core matches on Item ended in a catch-all arm, against the claim that each names every variant
- file: CHANGELOG.md
  line: 16
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the Breaking lines did not name the consumer cost
- file: crates/llm-core/tests/capabilities.rs
  line: 111
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a temperature/top_p setter swap survived; the adversary case kills it
- file: crates/llm-messages/src/codec.rs
  line: 130
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: role checks class an unknown kind as assistant content, refused in the same loop before I/O
- file: crates/llm-bedrock/src/invoke.rs
  line: 352
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: non-refusing catch-alls over Item sit behind refusing checks
- file: docs/harness-parity.md
  line: 306
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: historical advice to end a Capabilities literal with ..Capabilities::text no longer compiles
```
