---
format: aep.planning-md/3
id: review-result:adversary-w11-tokio-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:async-clients-without-a-tokio-runtime, pass 1: a turn moved out of its runtime still panics'
relations:
- reviews: story:async-clients-without-a-tokio-runtime
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 1 on `story:async-clients-without-a-tokio-runtime`, head
`b1c1ddd1` (base `a4835807`). The report as returned, paths outside the repository shortened.

```
unit: story:async-clients-without-a-tokio-runtime at b1c1ddd1 (plus 3 untracked adversary test files)
verdict: NEEDS-CHANGE
cases: executed 1032→1037, red 1
origin: introduced 2 / pre-existing 1 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: none
```

## Cases

- red `crates/llm-models/tests/adversary_w11_no_runtime.rs::a_turn_started_inside_a_runtime_and_finished_outside_is_refused_not_a_panic`:
  Responses panicked at `client.rs:356`, Messages at `decode.rs:812`, creating the per-event
  turn-deadline timer (also red at the base).
- green `…::a_routing_run_over_three_targets_outside_a_tokio_runtime_attempts_one_and_stops`
  (red at the base: a panic).
- green `crates/llm-http/tests/adversary_w11_stream_queue.rs`: a queued event is still returned
  outside a runtime and only the wire read is refused (kills a mutant the unit's tests miss); the
  check costs about 60 ns per call inside a runtime.
- green `crates/llm-tool-call/tests/adversary_w11_call_tool_no_runtime.rs`: `call_tool` outside a
  runtime gets the typed refusal before the login is read.

## Attacked and not broken

Routing takes no retry and no fallback on the refusal; every client's entry checks before projecting,
encoding and resolving; a stream polled outside a runtime is refused as `accepted`; current-thread
runtimes and both `BlockingModel` constructors are unchanged; the message leaks nothing.

```findings
- file: crates/llm-http/src/lib.rs
  line: 14
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: "the crate docs and docs/design.md:146 say a turn polled outside a runtime does not panic, but a turn started inside one and finished outside panics at the next turn-deadline timer"
- file: crates/llm-responses/src/client.rs
  line: 356
  category: acceptance
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: "a Responses or Messages turn moved out of its runtime mid-stream panics creating a timer (also llm-messages/src/decode.rs:812); built by the case, no caller found that reaches it"
- file: crates/llm-http/src/runtime.rs
  line: 7
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the refusal tells routing and call_tool callers to use b10x-llm-blocking, which cannot run either because neither is a Model"
- file: crates/llm-http/src/transport.rs
  line: 350
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the unit's tests stay green with the stream check moved above the queue; adversary_w11_stream_queue.rs now catches it"
```

## Routing

Back to the implementor: the check also runs before each per-event deadline timer, so the docs hold
and the red case passes; the message names both ways out (a Tokio runtime, or `b10x-llm-blocking`
for a single model); the adversary's three files stay in the unit.
