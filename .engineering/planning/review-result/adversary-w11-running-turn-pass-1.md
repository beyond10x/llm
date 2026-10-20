---
format: aep.planning-md/3
id: review-result:adversary-w11-running-turn-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:input-into-a-running-turn, pass 1: tool-call turns and Responses steering'
relations:
- reviews: story:input-into-a-running-turn
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 1 on `story:input-into-a-running-turn`, head `b257f198`
(base `5795b06e`). The report as returned, paths outside the repository shortened.

```
unit: story:input-into-a-running-turn at b257f198 (plus 1 untracked adversary test)
verdict: NEEDS-CHANGE
cases: executed 1053→1055, red 1
origin: introduced 6, pre-existing 0, undecided 0
wrote-outside-worktree: the assigned scratch (24 files, 1.9M; scratch build cleaned)
needs-coordinator: decision B assumes no protocol in scope accepts input into a running turn; OpenAI documents response.steer over Responses WebSocket mode
```

## Cases

In `crates/llm-core/tests/adversary_w11_interrupt_pattern.rs`, each building the next request as
the docs say and running `TurnRequest::validate_for`:

- red `a_turn_that_ended_in_a_tool_call_before_the_cancel_reached_it_continues_into_a_sendable_turn`:
  "every prior tool call needs a result before the next turn".
- green `the_same_turn_cancelled_part_way_continues_into_a_sendable_turn` (control).

The example's own test killed 1 of 4 mutants (dropping the finished-turn carry, removing the
empty-shown guard, a sink losing the delta it cancels on all stayed green); a stronger module kills
all four.

## Attacked and not broken

Every other citation in the new section; no event reaches the sink after a cancel on Responses and
Messages, and Chat delivers only the rest of the payload it last read, as the guide says; a
cancelled Responses turn leaves no state for the next turn; dropping thinking from the assistant
item is allowed outside tool use; a cancelled turn takes no retry or fallback.

```findings
- file: docs/design.md
  line: 125
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "for a turn that finished before the cancel landed, the documented next turn (outcome.items, then new input) is refused before sending by every client when that outcome ends in a tool call"
- file: website/docs/guides/interrupt-a-turn-with-new-input.md
  line: 13
  category: contract-drift
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the claim that no Responses wire accepts input into a streaming answer contradicts OpenAI's docs (response.steer over Responses WebSocket mode, GPT-6 family, fetched 2026-10-20); only llm's HTTP/SSE client lacks it"
- file: crates/llm-docs/examples/interrupt_turn.rs
  line: 201
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the example's only test kills 1 of 4 mutants because its expected value is built from first.shown"
- file: docs/design.md
  line: 104
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the vocabulary.rs citations are one line too high in this tree, moved by the unit's own doc-comment edit"
- file: website/docs/guides/interrupt-a-turn-with-new-input.md
  line: 289
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "'assistant text is exactly the text shown' contradicts design.md and port.rs for a Responses message with no deltas or unpaired shown text"
- file: website/docs/guides/interrupt-a-turn-with-new-input.md
  line: 283
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "Responses and Messages race the cancel against sink.emit and drop an emit in progress; the guide should say to record shown text only once emit completes"
```

## Routing

All six back to the implementor. Decision B stands with corrected reasons: llm's Responses client
speaks HTTP with SSE and does not implement WebSocket mode, so llm does not offer steering;
offering it is filed as `story:responses-websocket-steering` (draft).
