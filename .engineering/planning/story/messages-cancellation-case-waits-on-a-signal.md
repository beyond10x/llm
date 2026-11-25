---
format: aep.planning-md/3
id: story:messages-cancellation-case-waits-on-a-signal
kind: story
status: active
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
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:40Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:40Z", actor: "human:operator", revision: 3}
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

Derived 2026-11-25 by `story-scoper`, read at `main` 831c21d95. Every line is **cited** (read from the story or the tree; the source is named in brackets) or **inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-messages/tests/client.rs` — cited (story)
- **Files, the case:** `crates/llm-messages/tests/client.rs:412` `cancellation_reaches_a_caller_whose_sink_never_accepts`; the fixed wait is the `stop` future `:432-435` (`tokio::time::sleep(Duration::from_millis(100))` at `:433`, `cancel.cancel()` at `:434`); the assertions to keep are `:439-441` — cited (story, tree)
- **Files, what it runs against:** no shared fake; an inline loopback server spawned at `:414-426` on a `TcpListener`, with the file's own helpers `read_request` `:38-60`, `accept` `:100-105` and `listener` `:107-111`. `crates/llm-messages/tests/support/mod.rs` holds only the fixture binding and resolver — cited (tree)
- **Symbols, points where a signal could be handed to the test:**
- server, after the head is written: between `:422` and `:423` — cited (tree)
- server, after the whole `STREAM` is written: between `:423` and the 30 s hold at `:425` — cited (tree)
- client side, the sink: `BlockedSink::emit` `:404-409`, first called for the `TextDelta` of `STREAM` (`:29-30`) and pending from then on — cited (tree)
- client side, the transport: `HttpClient::with_response_tap` `crates/llm-http/src/transport.rs:160`, whose `head` is called at `:348-352` once the head was read; no Messages test installs one — cited (tree)
- precedent in the crate for a sink that signals: `AcceptingSink` with a `oneshot::Sender` `crates/llm-messages/tests/live_connection.rs:168-192`, awaited by the canceller `:212-216`; tokio's `sync` feature is on (`Cargo.toml:45`) — cited (tree)
- **Symbols, where the two dispatches come from:** a cancel before `execute` returned is `Cancelled` with `Dispatch::Unknown` and no observation, `crates/llm-http/src/transport.rs:339-341`; a cancel while the sink holds an event is `Cancelled` with `Dispatch::Accepted`, `crates/llm-messages/src/decode.rs:1117-1119`, and the snapshot is attached at `crates/llm-messages/src/client.rs:349-352` — cited (tree)
- **Files, the same defect elsewhere in the crate's tests (an assertion that fails when the head is late):**
- `crates/llm-messages/tests/client.rs:487` `the_turn_deadline_bounds_a_sink_that_never_accepts`: a 300 ms turn limit (`:506`), then dispatch `Accepted` and a retained snapshot asserted (`:519-523`); a head later than 300 ms is `Deadline` with `Dispatch::Unknown` (`crates/llm-http/src/transport.rs:344`). It is the only other one — inferred
- **Files, other cases under `crates/llm-messages/tests/` that end a turn or assert after a fixed wall time:**
- deadline cases that assert the code only, so a late head changes nothing: `adversary_client.rs:102` `the_absolute_turn_deadline_bounds_credential_resolution` and `:131` `the_absolute_turn_deadline_bounds_a_sink_that_never_accepts` (300 ms limits `:40-42`, 3 s outer bound); `adversary2_deadline.rs:99` `the_turn_deadline_bounds_a_stream_that_stops_arriving` and `:136` `the_turn_deadline_bounds_a_response_that_never_arrives` (300 ms `:34`, 3 s `:36`); `client.rs:457` `the_turn_deadline_bounds_credential_resolution` (300 ms, `NotSent` whatever the load) — cited (tree)
- waits that assert nothing arrived, which pass at every load and can only miss a late arrival: `client.rs:198` in `a_truncated_stream_keeps_its_partial_usage_and_is_not_sent_again` (150 ms), `:273` in `an_unresolvable_credential_never_reaches_the_network`, `:376` in `every_pre_flight_refusal_is_reached_through_the_client_before_anything_is_sent` (five times), `:479` in `the_turn_deadline_bounds_credential_resolution` (100 ms each); `retry_class.rs:398` in helper `relayed_turn`, used by `a_gateway_502_that_says_do_not_retry_is_final_and_without_it_is_retriable`; `live_connection.rs:103` in helper `serve`, used by both cases of that file; `adversary_subscription.rs:162` in `a_redirect_to_another_host_never_receives_the_subscription_token` (300 ms); `adversary_w26_image_input.rs:83` in helper `serve` (50 ms), used by the five `…_is_refused_and_nothing_is_sent` cases — cited (tree)
- wall-time bounds on a turn that should end by itself: `live_connection.rs:142` `a_connection_that_keeps_sending_after_message_stop_ends_the_turn_at_the_stop` (2 s `:149`, server patience 3 s `:48`, 5 ms pacing `:82`); `live_connection.rs:199` `a_cancel_while_the_server_still_sends_to_an_accepting_sink_stops_the_turn` (cancels on a signal already; asserts under 1 s after the cancel `:224`, 5 s outer `:217`); `adversary_w26_image_input.rs:51` in helper `received` (1 s read) — cited (tree)
- not waits before an assertion: the server holds `client.rs:425`, `:500`, `adversary_client.rs:145`, `adversary2_deadline.rs:112`, `:142`; the pre-cancelled token at `streaming.rs:2394` in `a_held_cost_the_caller_does_not_take_ends_the_turn_as_that_failure` — cited (tree)
- no file of the crate's tests calls `thread::sleep`; the only cancel after a sleep is the story's case — cited (tree)
- **Other crates, cancel after a fixed sleep (list only):**
- `crates/llm-responses/tests/adversary_w23_context_length.rs:307` `a_cancellation_while_a_400_body_is_awaited_ends_the_wait_at_once`: 150 ms (`:314`) through helper `turn_against` (`:156-157`); it asserts no dispatch, so a late head passes — cited (tree)
- `crates/llm-chat/tests/`: none; no file there calls `.cancel()` or `sleep` — cited (tree)
- `crates/llm-bedrock/tests/`: none; `credentials.rs:490` `a_caller_that_cancels_its_wait_leaves_the_fetch_to_finish` and `process_credentials.rs:805` `dropping_the_source_kills_the_command_it_is_running` cancel after a counter is reached (polled every 10 ms, `credentials.rs:196-203`, `process_credentials.rs:325-332`), and the other cancels are made before the call — cited (tree)
- **Also likely:** `crates/llm-messages/tests/support/mod.rs`, only if the signalling sink is shared between files; the six other files above, only if acceptance line 3 is met by changing them and not by naming each with its reason — inferred
- **Documents:** none has to change. The case is named outside its file in:
- `docs/harness-parity.md:191` (M12) and `:225` (M46), as `llm-messages/tests/client.rs:412`; that page cites tag `0.7.0`, where the case is at `:412` too, so it is left alone — cited (tree)
- `docs/verification/messages-falsification.json:267`, by name only, with `the_turn_deadline_bounds_a_sink_that_never_accepts` at `:268`; valid while the names stay — cited (tree)
- `docs/plan/wave-2026-11-25-messages-empty-turn.md:172-174` and `docs/plan/trajectory.md:107-108` (the story as a draft), wave records — cited (tree)
- `crates/llm-messages/tests/adversary_client.rs:10`, a module comment, by name — cited (tree)
- `.engineering/planning/review-result/adversary-w30-messages-empty-turn-pass-1.md:65` and `:100-103`, the finding — cited (tree)
- `AGENTS.md` (the invariants table included), `checks/`, `website/`, `spec/` and `CHANGELOG.md` name neither the case nor `crates/llm-messages/tests/client.rs` — cited (tree)
- **Confidence:** high for where the work lands, because the story names the file and the case and every line above was read; medium for which signal meets the acceptance, because line 1 names the server and the reading below says the server's signal is not enough — inferred
- **Would collide with:** any unit that edits or moves `crates/llm-messages/tests/client.rs`; any unit that moves the files of `crates/llm-messages/tests/` into one `tests/main.rs` or rewrites `mod support;`; any unit that edits `crates/llm-messages/tests/support/mod.rs`, if the sink is shared — inferred
- **Overlap with `story:test-binaries-one-per-crate`:** both touch `crates/llm-messages/tests/client.rs` (this story edits `:404-442` and perhaps `:487-524`; that one moves the file and rewrites `mod support;` at `:2` and the `use support::…` at `:19`), which is in that story's proposed part 1. They also both touch `crates/llm-messages/tests/support/mod.rs` and the six other files above if this story changes them. `AGENTS.md` is not shared. This story's Verification names `--test client`; after the other story that target is `--test main` with the case as `client::cancellation_reaches_a_caller_whose_sink_never_accepts`, so the command depends on the order — inferred
- **Safety fact:** the change is safe only if the cancel waits on something the client did, not on something the server wrote. `write_all` returning on the server (`client.rs:417-423`) says the bytes left the server, not that the client read them. The `#[tokio::test]` at `:411` runs on the current-thread runtime (the workspace tokio has no `rt-multi-thread`, `Cargo.toml:45`); the test task woken by a server signal can cancel before the client's connection task has read the head, and `crates/llm-http/src/transport.rs:339-341` then answers `Dispatch::Unknown`, the failure the adversary saw. A signal sent from `BlockedSink::emit` is given only after the head was read and the first delta decoded, and the cancel then lands in `crates/llm-messages/src/decode.rs:1117-1119`, which keeps what the case's name says it tests. A response tap proves the head was read, but the cancel can then land before the sink blocks. Step 3, walked by reading, unproven — inferred
- **Safety fact, second:** the fixed 100 ms sleep has been in the case since commit `f4be1dbda` (`git log -S`, one commit), so the wave that saw the failure did not add it; whether the `Unknown` branch of the transport is as old was not read. Step 2, unproven — inferred
