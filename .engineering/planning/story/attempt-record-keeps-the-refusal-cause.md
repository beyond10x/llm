---
format: aep.planning-md/3
id: story:attempt-record-keeps-the-refusal-cause
kind: story
status: implemented
title: A failed attempt's record keeps the refusal cause
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-30T13:37:15Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-30T14:17:45Z", actor: "human:unknown", revision: 3, executor: "agent:takt"}
- {from: "active", to: "implemented", at: "2026-10-30T14:35:23Z", actor: "agent:takt", revision: 4, decided_on: {"recorded":{"test_result":1}}}
---
## Context

Found by the adversary of `story:context-length-refusal-is-typed` in wave 2026-10-29-w23
(`review-result:adversary-w23-context-length-pass-1`, finding F5). `AttemptResult::Failed`
(`crates/llm-routing/src/fallback.rs:545`) keeps an attempt's code, dispatch and observation but not
`Error::refusal`, so the per-attempt record of a run loses the cause; only the run's final error
keeps it. Measured by reading the code; no consumer was shown to read the per-attempt record for
the cause.

## Outcome

A failed attempt's record carries the refusal cause its error named.

## Acceptance

- A routed run whose attempt is refused with `RefusalCause::ContextLengthExceeded` lists that cause
  on the attempt's record as well as on the final error.
- A refusal with no cause records none.

## Out of scope

- New causes.
