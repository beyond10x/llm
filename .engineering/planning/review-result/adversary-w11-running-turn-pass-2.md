---
format: aep.planning-md/3
id: review-result:adversary-w11-running-turn-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:input-into-a-running-turn, pass 2: one test gap, three wordings'
relations:
- reviews: story:input-into-a-running-turn
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 2 on `story:input-into-a-running-turn`, head `54503ab3`
(pass 1 at `b257f198`). The report as returned, paths outside the repository shortened.

```
unit: story:input-into-a-running-turn at 54503ab3 (plus 1 untracked adversary test)
verdict: PASS
cases: executed 1058→1060, red 0
origin: introduced 4, pre-existing 0, undecided 0
wrote-outside-worktree: the assigned scratch (476K; scratch build cleaned)
needs-coordinator: no
```

## Cases

`crates/llm-docs/tests/adversary_w11_interrupt_wire.rs`, green: the corrected next turn, built as the
example builds it, is sent by the real Responses, Messages and Chat Completions clients in each
provider's order for six endings (cancelled part-way, before any text, finished, finished in one
call, in two calls, with reasoning and a call); a control places the new input between a call and
its result and each wire's order check refuses it.

Example mutants: 6 of 7 killed by the example's tests; n6 (only the first proposed call answered)
survives because no test proposes two calls.

## Attacked and not broken

The pattern on every wire and ending; each encoder's rejected-result form; the steering text against
OpenAI's guide; the cancel-and-emit wording; the citations.

```findings
- file: crates/llm-docs/examples/interrupt_turn.rs
  line: 350
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "answering only the first proposed call leaves all four example tests green because none proposes more than one call, while design.md says they check a rejected result for each call"
- file: website/docs/guides/interrupt-a-turn-with-new-input.md
  line: 14
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "'every model declares input_during_output: false' says more than the code; a Model built in code keeps its own claim; it holds for every client in llm"
- file: website/docs/guides/interrupt-a-turn-with-new-input.md
  line: 51
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "'Any Model goes in its place' no longer holds now that the first request offers a tool; a model declaring tools = false refuses that turn before sending"
- file: docs/design.md
  line: 107
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the sentence reads as if Chat Completions were Anthropic's; it is OpenAI's API"
```

## Routing

All four back to the implementor (round 2); the adversary's wire test stays in the unit. No third
pass: two is the wave's limit, and round 2 adds a test and three wordings.
