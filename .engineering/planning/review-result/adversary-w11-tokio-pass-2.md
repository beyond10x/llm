---
format: aep.planning-md/3
id: review-result:adversary-w11-tokio-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:async-clients-without-a-tokio-runtime, pass 2: notes only'
relations:
- reviews: story:async-clients-without-a-tokio-runtime
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 2 on `story:async-clients-without-a-tokio-runtime`, head
`74ad3b12` (pass 1 at `b1c1ddd1`). The report as returned, paths outside the repository shortened.

```
unit: story:async-clients-without-a-tokio-runtime at 74ad3b12 (plus 3 untracked pass-2 test files)
verdict: CONFIRMED (notes only; nothing holds the unit)
cases: executed 1041→1045, red 0
origin: introduced 3 / pre-existing 1 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: none
```

## Cases

All green on `74ad3b12` and kept in the unit: a `post_json` body read moved out of its runtime is
refused as `accepted` (kills the mutant reporting `unknown`); `decode_stream` runs to its outcome on
an executor that is not Tokio (kills the mutant that always asks for a runtime); a decoder given a
deadline is refused outside a runtime; a turn whose runtime was dropped is refused where it waits
and drops cleanly. Suite: 1045 passed, 0 failed, 3 ignored.

## Attacked and not broken

Every timer is created inside the check; cancellation and deadlines are unchanged inside a runtime;
each refusal's dispatch gives routing no retry and no fallback; nothing that should run without a
runtime is guarded.

```findings
- file: crates/llm-http/src/transport.rs
  line: 593
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the unit's suite stays green when the post_json body-read refusal reports dispatch unknown instead of accepted; adversary_w11_pass2_json_body.rs now catches it"
- file: crates/llm-messages/src/decode.rs
  line: 811
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the unit's suite stays green when the Messages decoder asks for a runtime without a deadline, which breaks decode_stream on other executors; adversary_w11_pass2_decoder.rs now catches it"
- file: docs/design.md
  line: 187
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: "a turn moved into a second runtime after the first was dropped can still panic, which the doc's one no-panic measurement does not mention; no caller is known to do this"
- file: docs/design.md
  line: 167
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the doc says events already decoded are still handed over, but Responses and Messages refuse handing them to the sink outside a runtime; only Chat Completions hands them over"
```

## Routing

The coordinator kept the four cases and corrected both `docs/design.md` passages.
