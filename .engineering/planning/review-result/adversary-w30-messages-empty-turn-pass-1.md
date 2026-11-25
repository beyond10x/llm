---
format: aep.planning-md/3
id: review-result:adversary-w30-messages-empty-turn-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:messages-empty-turn-with-an-unmodelled-stop-reason, wave 2026-11-25-messages-empty-turn): 4 introduced, 1 undecided, 6 red cases'
relations:
- reviews: story:messages-empty-turn-with-an-unmodelled-stop-reason
revision: 1
---
Recorded by the coordinator as the adversary returned it; nothing is changed. Covers head `4c770f4bd`
of `impl/messages-empty-turn`.

unit: story:messages-empty-turn-with-an-unmodelled-stop-reason
verdict: red
cases: executed 364→371, red 6
origin: introduced 4, pre-existing 0, undecided 1
wrote-outside-worktree: session scratch only, `w30/u1/adv/` (scripts, logs, `orig/`, `mut/`)
needs-coordinator: yes

Covers head `4c770f4bd33e2b1480058312546df602cbce5eba` plus my uncommitted test edits. Rust 1.98.0.
Scenario lane not run (no scenario added).

```
 crates/llm-bedrock/tests/client.rs         |  37 +++++++++++
 crates/llm-messages/tests/retry_class.rs   |  86 ++++++++++++++++++++++++
 crates/llm-messages/tests/streaming.rs     |  37 +++++++++++
 crates/llm-messages/tests/usage_reader.rs  |  96 +++++++++++++++++++++++++++
 crates/llm-models/tests/relayed_failure.rs | 103 +++++++++++++++++++++++++++++
 5 files changed, 359 insertions(+)
```

Every path is a test file; `git status --short` shows these five and nothing else; `rustfmt --check`
exit 0.

**Cases added** (7; each run alone before the suite; logs `case-*.log`)

- C1 RED `usage_reader.rs:240`
  `adversary_w30_a_stream_that_stops_without_content_fails_with_the_terminal_counters_the_reader_reports`:
  `left: true / right: false` (reader `final_usage` against the decoder error's).
- C2 RED `usage_reader.rs:270`
  `adversary_w30_a_complete_message_that_stops_without_content_fails_with_the_terminal_counters_the_reader_reports`:
  `left: true / right: false`.
- C3 RED `crates/llm-models/tests/relayed_failure.rs:420`
  `adversary_w30_a_stop_at_the_context_window_without_content_is_priced_from_its_terminal_counters`:
  `left: (None, Some(QuantityUnknown)) / right: (Some(Amount(4000000000)), None)`.
- C4 RED `crates/llm-bedrock/tests/client.rs:1256`
  `adversary_w30_a_whole_invoke_answer_that_stops_without_content_is_final_as_the_retry_table_says`:
  panics on `pause_turn` with `Error { code: Unavailable, … retriable: true …}`.
- C5 RED `retry_class.rs:757`
  `adversary_w30_a_stop_without_content_after_a_costed_header_is_retried_as_an_overloaded_error_is`:
  `left: (Unavailable, false, false, 1) / right: (Unavailable, true, true, 0)` (code, retriable,
  may-retry, sink events).
- C6 RED `retry_class.rs:790`
  `adversary_w30_a_stop_without_content_whose_terminal_delta_reports_a_cost_is_retriable`: panics on
  `pause_turn` with `retriable: false`.
- C7 GREEN `streaming.rs:1825` `adversary_w30_the_refusal_row_is_chosen_by_its_whole_name`: guard for
  finding 4; red on both surviving mutants with
  `left: (Refused, Accepted, None) / right: (Unavailable, Accepted, None)` (`guard-*.log`).

**Suite** (`-p llm-messages`, `-p llm-bedrock --test client`, `-p llm-models --test relayed_failure`;
logs `suite-*.log`)

- Before (`--skip adversary_w30`): 328 + 31 + 5 = 364 executed, 1 failed:
  `cancellation_reaches_a_caller_whose_sink_never_accepts` (finding 5; not one of mine).
- After: 333 + 32 + 6 = 371 executed, 6 failed, all six are C1-C6; the cancellation case passed in
  this run. The count moved by the 7 cases added.

**Findings**

1. `crates/llm-messages/src/decode.rs:152` (`Header::attach`, used by `stopped_without_content`
   :195). Measured: the new failure carries `final_usage: false` although the stream reached
   `message_stop` or the whole body was read; `read_usage` says `true` for the same bytes (C1, C2),
   against the header of `tests/usage_reader.rs` ("must report what the turn decoder reports").
   Through `llm-calls` and `llm-cost` the 4000 reported input tokens become `QuantityUnknown` and the
   cost incomplete (C3; `crates/llm-cost/src/pricing.rs:187`; `docs/pricing.md` "A failed attempt can
   have final usage"). Reaches: every recorded call with a price book whose route answers such a
   stop, that is the unit's own case. Origin introduced: the failing path is created by the diff; at
   base the same bytes decoded `Ok` (read from the diff, not run on the base).
2. `crates/llm-messages/src/decode.rs:418` (also :809, :853). Measured: on a route that reports
   `usage.cost` the third row is never retried (C5, C6). The terminal `message_delta`, which
   `docs/messages.md:168-170` lists among the events that mean "the route answered nothing", releases
   the held cost as a `usage-extension` warning, and `finish` then strips the mark; `overloaded_error`
   after the same header drops the held cost and stays retriable. The unit pins this itself
   (`a_stop_without_content_after_a_shown_usage_extension_is_final`) and documents it
   (`docs/messages.md:171`), so the sentence "retried as `overloaded_error` is" and acceptance line 2
   hold only on routes without the extension. Reaches: an Anthropic-compatible route was observed
   sending `message_delta.usage.cost`; that route answering an unmodelled stop without content was
   not observed. Needs the coordinator: decision 3 against acceptance line 2.
3. `docs/bedrock.md:286`. Measured: the table (unchanged by the unit) says "Any failure while a whole
   (unstreamed) answer is read | final"; a whole `InvokeModel` answer that stops without content is
   now retriable with its billed usage attached (C4). `crates/llm-bedrock/src/client.rs:457-462`
   strips the mark on that path because a whole answer is "generated, and billed, before its first
   byte"; the new failure passes beside it, while `docs/messages.md:173` says InvokeModel shares the
   rule. Reaches: every unstreamed InvokeModel call under the retry policy; each retry is billed.
   Needs the coordinator: a table row, or the mark stripped on the whole path.
4. `crates/llm-messages/src/decode.rs:200`. Measured: mutants `r if r.starts_with("refusal")` and
   `r if "refusal".starts_with(r)` passed the unit's three targets (baseline green, 34 + 14 + 62).
   The unit tests `REFUSAL` and `model_context_window` only. C7 kills both.
5. `crates/llm-messages/tests/client.rs:440`. Measured: failed once in the suite's before-run
   (`left: Unknown / right: Accepted`), passed in the after-run and 40 of 40 times alone
   (`flake.status`). A fixed 100 ms sleep races the response head. The unit changed neither this file
   nor `src/client.rs`; not run on the base, so undecided.

Judgement, no case: a Chat `content_filter` answer with no content relayed through Messages ingress
is now `unsupported` for the client (decision 4, in the CHANGELOG as breaking);
`docs/implementation-status.md` has no row for the breaking change; the CHANGELOG has 7 bullets for
this one change.

**Attacked and not broken**: no-item detection on a stream whole and byte-wise and on
`decode_message`; a block left open stays `protocol`; the seven other mutants were killed
(`rows-swapped` 9 failed cases, `no-content-block-instead-of-no-item` 1,
`delta-extension-not-counted` 1, `held-extension-not-counted` 2,
`encoder-refusal-dropped-for-a-message` 1, `encoder-refusal-dropped-for-a-stream` 1,
`window-row-also-for-any-window-name` 6). By reading only, not measured: `error` event or cut after
the terminal delta stays final; reason validation (empty, over 64 bytes, non-string) is the earlier
`protocol` refusal; fallback count over two targets; each failed attempt is recorded and priced once.

```findings
- file: crates/llm-messages/src/decode.rs
  line: 152
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a stop without content fails with final_usage false although the answer ended, so the usage reader disagrees and the billed input tokens are priced as unknown
- file: crates/llm-messages/src/decode.rs
  line: 418
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: on a route that reports usage.cost the terminal delta releases the held cost and the retriable row is always final, unlike overloaded_error after the same header
- file: docs/bedrock.md
  line: 286
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the retry table makes every failure of a whole answer final, and a whole InvokeModel answer that stops without content is now retriable with billed usage
- file: crates/llm-messages/src/decode.rs
  line: 200
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: matching the refusal row by prefix in either direction survived the unit's tests until the added guard case
- file: crates/llm-messages/tests/client.rs
  line: 440
  category: concurrency
  severity: note
  verdict: CONFIRMED
  origin: undecided
  message: the cancellation case failed once of 42 runs with dispatch Unknown because a fixed 100 ms sleep races the response head
```

## What the coordinator decided

1. Fixed in correction round 1: the failure carries final usage when the answer ended.
2. Decision 3 of the implementor is reversed: a turn that fails by this rule shows no usage-extension
   warning; the held cost is dropped as for `overloaded_error`, so the third row stays retriable.
3. The existing rule wins: a whole InvokeModel answer is final; the streamed path keeps the row.
4. The guard case stays.
5. Not this unit's: filed as a draft story.
