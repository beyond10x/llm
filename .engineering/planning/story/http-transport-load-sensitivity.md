---
format: aep.planning-md/3
id: story:http-transport-load-sensitivity
kind: story
status: active
title: The HTTP transport deadline test passes under load
relations:
- serves: vision:portable-model-inference
- decomposes: epic:contracts
scope:
- confidence: cited
  path: crates/llm-http/tests/transport.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T21:34:43Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-30T12:14:44Z", actor: "human:unknown", revision: 6, executor: "agent:takt"}
---
## Acceptance

`headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request`
(`crates/llm-http/tests/transport.rs`) passes under load, checked at two levels:

1. 10 of 10 runs of `cargo test -p llm-http --test transport
   headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request` with the test process
   pinned to one CPU (`taskset -c 0`) beside 4 CPU-bound workers pinned to the same CPU under
   `nice -n 19` (the workspace's limit for a load generator: at most 4 workers, nice 19, 10 runs).
2. 10 of 10 runs of the same test in a scratch copy whose `response_headers` bound is lowered from
   50 ms to 1 ms, which forces the case the observed failure suggests: the client gives up before
   the request is fully written. Recorded as the run's evidence, not committed.

The bound the test waits on is one the test sets. The fix stays in the test: no change to
`crates/llm-http/src/transport.rs`, to when `response_headers` starts, or to the transport
specification. If the fix needs either, the story stops and is re-scoped.

## Observed

2026-09-27, wave 3 llm gate, load average 45 on 20 cores: panicked at
`crates/llm-http/tests/transport.rs:16:9` `assertion failed: length > 0`, then `:207:18`
`JoinError::Panic`. The same tree passed it 5 of 5 alone and in the next full `task check`.

## Scope

Refreshed 2026-10-29 at `bb3bda33` (llm 0.17.1); confidence high.

- One file: `crates/llm-http/tests/transport.rs`, test
  `headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request` (`:175-208`; the
  50 ms bound at `:190`, the worker's `request` call at `:179`) and its helper `request`
  (`:11-33`, the `assert!(length > 0)` at `:16`). cited
- Hypothesis, not reproduced: the `response_headers` bound also covers connect and send
  (`crates/llm-http/src/transport.rs:312` and `:723`, `headers_by = deadline.min(started +
  self.limits.response_headers)` around `client.execute`), so under load the client closes
  before the request is written and the helper panics on EOF. inferred
- The fix stays in the test.
- Collides with any unit editing `crates/llm-http/tests/transport.rs`; the helper has 12 call
  sites there.
