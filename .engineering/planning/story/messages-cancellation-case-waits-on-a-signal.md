---
format: aep.planning-md/3
id: story:messages-cancellation-case-waits-on-a-signal
kind: story
status: draft
title: The Messages cancellation case waits on the response head, not on a fixed sleep
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w30-messages-empty-turn-pass-1
- decomposes: epic:inference
revision: 1
---
## Context

`crates/llm-messages/tests/client.rs`, `cancellation_reaches_a_caller_whose_sink_never_accepts`
(near :440), cancels the turn after a fixed `tokio::time::sleep` of 100 ms and then asserts dispatch
`Accepted`. When the response head has not arrived within those 100 ms, the dispatch is `Unknown` and
the case fails.

The adversary of wave 2026-11-25-messages-empty-turn saw it fail once in a suite run under load
(`left: Unknown / right: Accepted`) and pass in the next run and 40 of 40 times alone. The wave
changed neither that file nor `src/client.rs`; the case was not run on the wave's base, so its origin
is not established.

## Outcome

The case cancels when the server has sent its response head, not after a wall time, and passes at
every load.

## Acceptance

- The case holds no `sleep` and no wall-time bound: the cancellation waits on a signal the loopback
  server gives after it wrote the head.
- The case's assertions are unchanged: `Cancelled`, dispatch `Accepted`, the snapshot retained.
- The other cases of `crates/llm-messages/tests/` that wait a fixed time before an assertion are
  listed in the story's close, each changed the same way or named with its reason.

## Verification

Targeted: `-p llm-messages --test client` under nextest with the 60 s bound, run 100 times while a
second build holds the pool's slots. No live call.
