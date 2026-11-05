---
format: aep.planning-md/3
id: review-result:adversary-w26-feature-capabilities-pass-1
kind: review-result
status: active
title: Wave 2026-11-05-w26 adversary pass, llm story:output-and-image-capabilities, pass 1
relations:
- reviews: story:output-and-image-capabilities
revision: 1
---
```
unit: beyond10x/llm story:output-and-image-capabilities, commit 11520b4ef
verdict: CONFIRMED (two warnings, one note), overall GREEN
cases: executed 57→69, red 0
origin: introduced 3 / pre-existing 0 / undecided 0
```

The implementation scans every item, and holds the lead's rules: a declared `false` is rejected by
name and never a fallback, an undeclared feature is not rejected, `validate_for` refuses a declared
`false` before I/O, rejection order is fixed, the refusal carries no turn bytes, and spec, schema and
code agree on `RouteRejection`. The gap was in the tests: every unit test and all 8 scenarios put the
image last, so a check of the last item alone passed the whole suite and all 1013 scenarios. The
adversary's twelve cases close it (d6cf1e8cc, merged 5beb72c39).

```findings
- file: crates/llm-routing/src/selection.rs
  line: 276
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a selection image check of the last item alone passed the unit suite; the adversary case catches it
- file: crates/llm-core/src/turn.rs
  line: 276
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: validate_for checking the last item alone passed the unit suite; the adversary cases catch it
- file: checks/conformance/src/routes.rs
  line: 73
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the llm.routes.Load capabilities projection omits the two declarations, so no routes scenario observes them
```
