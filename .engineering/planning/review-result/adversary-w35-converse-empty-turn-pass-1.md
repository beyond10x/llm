---
format: aep.planning-md/3
id: review-result:adversary-w35-converse-empty-turn-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason, wave 2026-11-25-converse-empty-turn): red, 3 warnings and 4 notes, 2 red cases'
relations:
- reviews: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
revision: 1
---
Adversary pass 1 on unit `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`, head
`af5169a2f3cebecdf488d9b591e386ebb3451e26`, base `7ddbd1d6db98985229d9a2236f73402fbd5ed835`.

Verdict: red. Cases executed 198 → 200, 2 red. Origin: introduced 6, pre-existing 1.

The two cases, in `crates/llm-bedrock/tests/client.rs`, are red at the head and green with the base's
`converse.rs` and `client.rs`:

```
adversary_w35_an_unlisted_stop_after_a_recorded_silent_text_block_is_not_marked_retriable panicked at crates/llm-bedrock/tests/client.rs:1704:13:
`stop_sequence`: a content block opened and closed before the stop, and the failure is marked for another attempt: Error { code: Unavailable, ... dispatch: Accepted, retriable: true, ... usage: Some(Usage { input_tokens: Some(7), output_tokens: Some(1), ... }), final_usage: true }
adversary_w35_a_stop_after_an_empty_text_block_reads_alike_on_both_wires panicked at crates/llm-bedrock/tests/client.rs:1727:9:
stream `a_reason_in_no_list`: Converse (left) and InvokeModel (right)  left: Err((Unavailable, true))  right: Ok(Incomplete { reason: "a_reason_in_no_list" })
```

Suite afterwards: `cargo test -p llm-bedrock --no-fail-fast` exit 101, 198 passed, 2 failed (the two
above), 18 binaries; `cargo test -p llm-models --test relayed_failure` exit 0, 9 passed;
`cargo run --locked -p llm-conformance -- check` exit 0, 1087 passed three times, skipped 0.

1. `crates/llm-bedrock/src/converse.rs:474`. The first three frames of the recorded
   `pydantic-test_bedrock_model_stream_empty_text_delta.bin` (a text block with no text, closed),
   then `messageStop` for `stop_sequence` or a name in no list, is `unavailable` with `may_retry()`
   true and one output token billed; the base returned an incomplete turn. `docs/design.md` says a
   block that opens empty has started answering and that after a block, visible or silent, there is
   no failure to retry. The block shape is recorded; the stop right after it is built by the case,
   and no recording in the repository holds an unlisted Converse stop. No code in the repository
   retries a Converse turn; a caller that follows `may_retry` asks again.
2. `docs/design.md:509`. "The table differs from the Messages one in its reason names only" and
   "counts the same way" are untrue: through one `BedrockClient` the same answer (an empty text
   block, then the stop) is `Ok(Incomplete)` on InvokeModel, where the Messages decoder counts the
   block, and `Err(unavailable)` or `Err(too-large)` on Converse, streamed and whole.
3. `crates/llm-bedrock/src/client.rs:343`. A mutant that emits the advisory warning for a failed
   turn too survives all 198 `llm-bedrock` cases, `relayed_failure` and the conformance check. Under
   it a turn with `prompt_cache_key` that stops without content shows `advisory-omitted` and then
   fails retriable; at the head it shows nothing. No test holds that for a request with a hint; the
   Messages unit has such a test. The same mutant survives for the older failures (cut, throttling),
   which is pre-existing.
4. `docs/bedrock.md:317`, `spec/domains/bedrock.yaml:137`, `CHANGELOG.md:15`. "a counter the answer
   left out stays unknown, never zero" contradicts `docs/bedrock.md:254` and `:260`: a failure whose
   `metadata` has only `cacheReadInputTokens` carries `cache_creation_input_tokens: Some(0)`, and
   the unit's own test expects `Some(0)` for cache counters. The code is as the older paragraph says.
5. `crates/llm-bedrock/src/converse.rs:889`. The doc comment of `has_answered` says "A failure after
   it is final"; at the head the stop without an item after `contentBlockDelta` and
   `contentBlockStop` is not.
6. `crates/llm-models/tests/relayed_failure.rs:388` (judgement). The `Rebound` sentence is true by
   reading. With the retriable mark removed the attempts test and the asks-once test go red, the
   final-row test stays green. The two adapter tests are evidence that routing's loop bounds an error
   with `may_retry`, which the Messages tests in the file already hold, and that the Converse error
   passes routing's checks; they are not evidence that a production path bounds a Converse retry,
   because nothing outside the tests puts a Converse client under the policy (`Routes::port` hands
   out the bare client, the CLI calls `turn` once).
7. `website/docs/guides/call-a-model-on-bedrock.md:133` and `crates/llm-bedrock/src/client.rs:264`,
   pre-existing. "After that every failure is final" was already untrue for InvokeModel at the base;
   the unit makes it untrue for Converse as well and leaves both sentences.

Mutants, 13, each in the mutants worktree: rows swapped, no retriable mark, the guard false, `whole`
keeping the mark, usage not final, no dispatch, no `content_filtered` row, the stream dropping the
mark: caught by the crate's tests and by conformance. The too-large row retriable, the text quoting
the reason, rows matched without case, an opaque-only answer counted as no item: caught by the
crate's tests, missed by conformance alone. The advisory mutant: missed by both (finding 3).

Attacked and not broken: no answer shows an event and leaves `items` empty; a reasoning block of a
signature only is an opaque item nobody was shown, incomplete at the head and at the base; the early
return skips no check that changes a class; non-null `additionalModelResponseFields` is
`unsupported` before the table, and no byte of it or of a `trace` reaches the error; no `metadata`
at all stays the final `protocol` refusal; `usage` absent gives unknown usage with `final_usage`
true; the conformance adapter reads the error's own code, dispatch, `may_retry()` and observation;
every test name on a changed document line exists; the data README holds; `eventstream.rs` takes
only `.bin`; no stale sentence in `README.md`, `website/docs/` or
`docs/implementation-status.md`; the suite is the generator's output by the check's own byte
comparison. Test binaries 18 → 18 and 19 → 19. nextest: `llm-bedrock` 3.6 s, slowest
`every_recorded_body_decodes_alike_at_every_split_into_two_chunks` 3.5 s; `llm-models` 85 cases
2.4 s; no case of the unit over 0.04 s.

```findings
- file: crates/llm-bedrock/src/converse.rs
  line: 474
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: a recorded empty text block followed by an unlisted stop fails as a retriable unavailable although the design says a silent block ends retry eligibility; the stop after that block is built, not recorded
- file: docs/design.md
  line: 509
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the sentence that the Converse table differs from the Messages one in its reason names only is untrue, an empty text block is an item on InvokeModel and no item on Converse through the same client
- file: crates/llm-bedrock/src/client.rs
  line: 343
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a mutant that shows the advisory warning before a stop without content survives the crate's tests, relayed_failure and the conformance check, so no test holds that such a turn with a hint shows nothing
- file: docs/bedrock.md
  line: 317
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the claim that a counter the answer left out is never zero contradicts the known 0 of a Converse cache counter stated at line 254 and returned on the failure
- file: crates/llm-bedrock/src/converse.rs
  line: 889
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the doc comment of has_answered still says a failure after it is final, which the new retriable stop after an empty block contradicts
- file: crates/llm-models/tests/relayed_failure.rs
  line: 388
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the two adapter tests are evidence for routing's loop and not for a production path that bounds a Converse retry, since no code outside tests puts a Converse client under the retry policy
- file: website/docs/guides/call-a-model-on-bedrock.md
  line: 133
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: the guide and the client's struct doc say every failure after the model answered is final, untrue for InvokeModel at the base and now for Converse too
```
