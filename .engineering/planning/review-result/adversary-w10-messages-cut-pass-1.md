---
format: aep.planning-md/3
id: review-result:adversary-w10-messages-cut-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w10 adversary, story:messages-cut-after-silent-output-is-final, pass 1: two Chat gaps at the base'
relations:
- reviews: story:messages-cut-after-silent-output-is-final
revision: 1
---
## Pass

Wave 2026-10-20-w10, `aep:adversary` pass 1 on `story:messages-cut-after-silent-output-is-final`
(widened to Chat), head `d8f79616` (base `9cd7dbe8`). The report as returned, paths outside the
repository shortened.

```
unit: messages-cut at d8f79616
verdict: PASS
cases: executed 1009→1016, red 2
origin: introduced 1, pre-existing 2, undecided 0
wrote-outside-worktree: the assigned scratch (14 logs, a source copy with its own target of 1.7G)
needs-coordinator: the 2 red Chat cases are pre-existing and turn the workspace suite red
```

## Cases

- red `crates/llm-chat/tests/adversary_w10_cut_count.rs::output_in_a_delta_field_the_projection_does_not_read_makes_a_cut_final`:
  a cut after `delta.refusal` or `delta.reasoning` text stays retriable (also red at the base).
- red `…::a_drop_after_the_done_sentinel_returns_the_complete_turn`: a body dropped after `[DONE]`
  fails the turn (retriable at the base, final now).
- green `…::chunks_without_output_keep_a_cut_retriable` (kills mutant M1, `answered = true` in
  place of the empty-array guard), `…::an_opening_chunk_with_whitespace_content_is_output`; Messages
  `a_cut_after_a_keep_alive_alone_stays_retriable`,
  `a_cut_after_a_message_delta_without_any_block_is_final`,
  `a_drop_after_the_terminal_event_returns_the_complete_turn`.

Suite with the cases (`--no-fail-fast`): 1013 passed, 3 failed, 3 ignored; the third failure was
`llm-http` `transport::headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request`
under load, 3 of 3 green alone (`story:http-transport-load-sensitivity`).

## Attacked and not broken

The Messages count (ping, message_start, message_delta, an error after a silent block); mutant M2
(`message_delta` added to the lifecycle events) is caught by an existing w09 case; no other `Err`
path in either loop carries a retriable mark; layer 1's 429 and `x-should-retry` cases return before
the stream loop; routing ends a cleared, accepted failure at `IneligibleFailure` (`fallback.rs:537`),
as the docs say; every moved parity citation lands.

```findings
- file: crates/llm-chat/src/incoming.rs
  line: 134
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: pre-existing
  message: "Chat counts output from a closed list of fields, so a cut after delta.refusal or delta.reasoning text stays retriable with nothing shown and routing retries a turn the model already answered"
- file: crates/llm-chat/src/client.rs
  line: 83
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: "the Chat stream loop reads past [DONE], so a connection dropped before the chunked terminator fails a complete turn that Messages would return"
- file: crates/llm-chat/src/incoming.rs
  line: 156
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "replacing the empty-array guard with answered = true passes the whole llm-chat suite; only adversary_w10_chunks_without_output_keep_a_cut_retriable catches it"
```

## Routing

- `incoming.rs:134` and `client.rs:83`: filed as `story:chat-counts-every-output-delta` (draft); the
  two red cases are kept outside the repository for it.
- `incoming.rs:156`: the adversary's green cases join the unit, which closes it.
