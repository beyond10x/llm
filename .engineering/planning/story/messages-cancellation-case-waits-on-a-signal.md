---
format: aep.planning-md/3
id: story:messages-cancellation-case-waits-on-a-signal
kind: story
status: implemented
title: The Messages cancellation case waits on the response head, not on a fixed sleep
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w30-messages-empty-turn-pass-1
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: crates/llm-messages/tests/client.rs
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:40Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:40Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T19:02:20Z", actor: "human:operator", revision: 7, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
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

## Scope

Read by the coordinator from `git diff --stat` of the integration branch against the wave's opening
commit (`45e7c1c13`) at the close of wave 2026-11-25-tests-without-a-clock: one file,
`crates/llm-messages/tests/client.rs` (cited), 155 insertions and 12 deletions. The section before
this one was derived by `story-scoper` on 2026-11-25.

What the unit did there:

- `cancellation_reaches_a_caller_whose_sink_never_accepts` cancels after two signals, the server's
  after it wrote the head and the sink's when `emit` is first entered. No `sleep` and no bound in
  time stands between the start of the turn and the cancel. Its three assertions are the base's.
- `the_turn_deadline_bounds_a_sink_that_never_accepts` starts at a 300 ms turn limit and doubles it
  while the sink was offered no event, to 9.6 s at most; it asserts on the attempt that reached the
  sink, and that the turn lasted at least its limit. It stays bound by time: a fix without wall time
  needs a deadline the caller can inject under `crates/llm-messages/src/`.
- `adversary_w36_a_cancel_wakes_a_turn_parked_in_the_sink`, the adversary's case: the cancel wakes a
  turn parked in the sink with no poll between.

The list the third acceptance line asks for, the other cases of `crates/llm-messages/tests/` that
wait a fixed time before an assertion, each named with its reason and not changed:

- deadline cases that assert the code only, so a late head changes nothing:
  `adversary_client.rs` `the_absolute_turn_deadline_bounds_credential_resolution` and
  `the_absolute_turn_deadline_bounds_a_sink_that_never_accepts`;
- waits that assert nothing arrived, which pass at every load:
  `client.rs` `a_truncated_stream_keeps_its_partial_usage_and_is_not_sent_again` (150 ms) and the
  cases of the same shape the section before this one lists;
- three cases keep a wall-time bound that is neither of the two:
  `live_connection.rs` `a_connection_that_keeps_sending_after_message_stop_ends_the_turn_at_the_stop`
  (2 s on the turn, 3 s server patience), `live_connection.rs`
  `a_cancel_while_the_server_still_sends_to_an_accepting_sink_stops_the_turn` (under 1 s after the
  cancel) and `adversary_w26_image_input.rs` `an_accepted_image_request_does_reach_the_listener`
  (a 1 s read gap). Each bounds something that should end by itself; none failed in this wave's
  runs.

Not touched: `crates/llm-messages/src/`, `tests/support/mod.rs`, every document, `CHANGELOG.md`.
