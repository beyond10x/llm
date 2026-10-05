---
format: aep.planning-md/3
id: review-result:adversary-w27-llm-parity-retry-classes-pass-1
kind: review-result
status: active
title: Wave 2026-10-05-w27 adversary, llm story:parity-retry-classes, pass 1
relations:
- reviews: story:parity-retry-classes
revision: 1
---
```
unit: llm/parity-retry-classes, findings cover 83a0903 plus 2 untracked adversary test files
verdict: red
cases: executed 643→646, red 3
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 3 logs in ~/.cache/ga-wave-2026-10-05-w27/llm-parity-retry-classes/scratch/
needs-coordinator: yes (F1 fix lands in llm-responses or R42 is downgraded)
```

Cases added: `crates/llm-responses/tests/adversary_w27_retry.rs` (an answered Responses turn cut mid-frame is offered for another attempt: `retriable: true`, `dispatch: Accepted`), `crates/llm-routing/tests/adversary_w27_retry.rs` (a retry wait past the deadline ends `Deadline` instead of falling back; a cancellation landing with a retriable failure still emits `turn-retried`). Suite: 643 passed, 3 failed.

| # | file:line | verdict | origin | finding |
|---|---|---|---|---|
| F1 | crates/llm-responses/src/client.rs:101 | NEEDS-CHANGE, blocker | introduced | the Responses client buffers the stream; an answered turn cut mid-frame or mid-body reaches routing with 0 visible events and `may_retry()` true and is replayed up to 3 times with unmeasured usage; R42 does not hold on this path |
| F2 | crates/llm-routing/src/fallback.rs:441 | NEEDS-CHANGE, warning | introduced | a retry wait that would pass the deadline halts `deadline` instead of falling back, contrary to spec/domains/routing.yaml |
| F3 | crates/llm-routing/src/fallback.rs:431 | CONFIRMED, note | introduced | `wait_to_retry` warns and pauses after the caller cancelled; Harness checks cancellation first |
| F4 | checks/conformance/src/fallback.rs:285 | CONFIRMED, note | introduced | the conformance program cannot set a future deadline, so no scenario reaches the wait-past-deadline rule |
| F5 | crates/llm-routing/src/fallback.rs:120 | CONFIRMED, note | introduced | `FallbackPolicy::disabled()` keeps four same-target attempts including unknown-dispatch replays |

Not broken: visible output from a failing attempt halts before retry; 400/401/403/409 and Unauthorized/Refused/Invalid/Deadline/Cancelled never retried; the limit check before every retry; retry-after parsing and cap; attempts = 1 + 3 retries, total targets × attempts; cancellation during a wait; the warning is not visible output; 30 parity citations.

```findings
[
{"file":"crates/llm-responses/src/client.rs","line":101,"category":"acceptance","severity":"blocker","verdict":"NEEDS-CHANGE","origin":"introduced","message":"ResponsesClient buffers the stream, so an answered turn cut mid-frame or mid-body reaches routing with no visible events and may_retry true, gets replayed with unmeasured usage, and R42 never-after-answering does not hold"},
{"file":"crates/llm-routing/src/fallback.rs","line":441,"category":"contract-drift","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"a retry wait that would pass the deadline halts the run with deadline instead of falling back to the next target, contrary to spec/domains/routing.yaml and to pre-unit 429 fallback"},
{"file":"crates/llm-routing/src/fallback.rs","line":431,"category":"concurrency","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"wait_to_retry emits turn-retried and requests a pause after the caller has already cancelled; Harness checks cancellation before deciding to retry"},
{"file":"checks/conformance/src/fallback.rs","line":285,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"the conformance program can only set a deadline that has already passed, so no scenario reaches the wait-past-deadline rule the spec states"},
{"file":"crates/llm-routing/src/fallback.rs","line":120,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"FallbackPolicy::disabled() keeps four same-target attempts including unknown-dispatch replays, which its name does not suggest"}
]
```
