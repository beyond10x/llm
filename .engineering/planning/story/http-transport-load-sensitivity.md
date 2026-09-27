---
format: aep.planning-md/2
id: story:http-transport-load-sensitivity
kind: story
status: draft
title: The HTTP transport deadline test passes under load
relations:
- serves: vision:portable-model-inference
- decomposes: epic:contracts
revision: 1
---
## Acceptance

`headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request`
(`crates/llm-http/tests/transport.rs`) passes under concurrent load. The bound it waits on is one
the test sets.

## Observed

2026-09-27, wave 3 llm gate, load average 45 on 20 cores: panicked at
`crates/llm-http/tests/transport.rs:16:9` `assertion failed: length > 0`, then `:207:18`
`JoinError::Panic`. The same tree passed it 5 of 5 alone and in the next full `task check`.
