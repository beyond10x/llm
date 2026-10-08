---
format: aep.planning-md/3
id: story:http-transport-load-sensitivity
kind: story
status: draft
title: The HTTP transport deadline test passes under load
relations:
- serves: vision:portable-model-inference
- decomposes: epic:contracts
scope:
- confidence: cited
  path: crates/llm-http/tests/transport.rs
revision: 3
---
## Acceptance

`headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request`
(`crates/llm-http/tests/transport.rs`) passes under concurrent load. The bound it waits on is one
the test sets.

## Observed

2026-09-27, wave 3 llm gate, load average 45 on 20 cores: panicked at
`crates/llm-http/tests/transport.rs:16:9` `assertion failed: length > 0`, then `:207:18`
`JoinError::Panic`. The same tree passed it 5 of 5 alone and in the next full `task check`.

## Scope

Scoped 2026-10-08 at 560f044c; confidence high.

- One file: `crates/llm-http/tests/transport.rs`, test `headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request` (`:175-208`) and its helper `request` (`:11-33`, the `assert!` at `:16`).
- Hypothesis, not reproduced: the 50 ms `response_headers` bound (`:190`) also covers connect and send (`crates/llm-http/src/transport.rs:213`, `:426`), so under load the client closes before the request is written and the helper panics on EOF.
- The fix stays in the test. Changing when `response_headers` starts would move the scope to `crates/llm-http/src/transport.rs` and the transport specification.
- Collides with any unit editing `crates/llm-http/tests/transport.rs`; the helper has 12 call sites there.
