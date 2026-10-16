---
format: aep.planning-md/3
id: review-result:adversary-w07-responses-ingress-encoder-pass-2
kind: review-result
status: active
title: Wave 2026-10-16-w07 adversary, story:responses-ingress-encoder, pass 2
relations:
- reviews: story:responses-ingress-encoder
revision: 1
---
unit: story:responses-ingress-encoder, tree `~/.local/state/worktree/trees/b10x/llm/llm-w07-responses-ingress-encoder` at head 35d4e035 plus one untracked test file
verdict: red
cases: executed 173→178, red 5
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths (both in the assigned scratch dir, listed in part 6)
needs-coordinator: yes. The call-between-deltas case cannot pass together with the committed pass-1 case `text_call_text` until a block boundary exists, so it is a story:ingress-block-boundaries question.

**1. What I touched.** `git --no-pager diff --stat` is empty because the new file is untracked. `git status --short` shows one path, and it is a test file:
```
?? crates/llm-responses/tests/adversary_w07_ingress_pass2.rs
```

**2. Cases added**, all in `crates/llm-responses/tests/adversary_w07_ingress_pass2.rs`. All five are red now, and all five were red the first time this file ran alone (exit 101). The output below is verbatim from that run:

| case | asserts | red output |
|---|---|---|
| `fail_does_not_turn_a_partial_usage_snapshot_into_final_counters` | a failure whose usage snapshot has `final_usage: false` must not decode back as final counters | `the partial snapshot Usage { input_tokens: Some(25), output_tokens: Some(1), .. } reads back as final counters: TurnObservation { .. usage: Some(Usage { input_tokens: Some(25), output_tokens: Some(1), .. }), final_usage: true }` |
| `one_message_with_reasoning_between_its_deltas_reads_back_whole` | events text, reasoning, text with an outcome of one `AssistantText` round-trip | `close refused a valid outcome: Error { code: Protocol, message: "model outcome contradicts what was already streamed", .. }` |
| `one_message_with_a_call_between_its_deltas_reads_back_whole` | events text, call, text with an outcome of one message plus the call round-trip | same refusal |
| `an_announced_call_without_argument_deltas_reads_back_as_the_events_encoded` | `[ToolCallStarted]` with arguments `{}` reads back as the same events | `left: [ToolCallStarted call-1, ToolArgumentsDelta "{}"]` / `right: [ToolCallStarted call-1]` |
| `an_empty_message_ahead_of_a_shown_one_is_not_a_contradiction` | an outcome of `["", call, "Checking."]` round-trips | same refusal |

**3. Suite run, after the cases existed.** Command: `RUSTUP_TOOLCHAIN=1.98.0 cargo test -p b10x-llm-responses --locked --no-fail-fast`. Exit 101. My binary: `test result: FAILED. 0 passed; 5 failed`. Every other binary: `ok`, 173 passed. The before count of 173 is this same run with my binary taken out. Clippy (`--all-targets -D warnings`) and rustfmt are clean on the new file.

**Pass-1 corrections hold.** All four pass-1 cases are green, and none of their assertions was dropped:
- The two-messages iteration is re-pinned as an exact refusal, per the decided limit: code `Protocol`, the message, `response.failed`, the events, and `is_err`.
- Every payload now uses the caller's id, so the fixture's `response_id` became `resp_gateway_1`. That matches the decided limit and relaxes no assertion.
- Non-object arguments are refused on both paths.

**4. Findings.** `src/ingress.rs` does not exist at base 6d889e7a, so every finding is `introduced`.

- **`ingress.rs:397` — `fail` reports a partial usage snapshot as final counters (blocker, NEEDS-CHANGE).**
  - What breaks: `fail` copies `error.observation.usage` into `response.failed` without checking `final_usage`. The decoder reads a failed response's usage back with `final_usage: true`. A mid-stream snapshot of 25 input / 1 output tokens therefore becomes a final cost, and `llm-cost/src/pricing.rs:214` then prices it as known. This breaks the "unknown usage stays unknown" invariant.
  - What reaches it: the Messages decoder attaches a snapshot with `final_usage: false` on a mid-stream `error` event (`llm-messages/src/decode.rs:123,662`). That snapshot holds the counters from `message_start`.
  - Fix to name: carry usage only when `observation.final_usage` is true.
- **`ingress.rs:186` — a reasoning delta splits one message (warning, NEEDS-CHANGE).**
  - What breaks: text after a reasoning delta opens a new message item, so `close` refuses an outcome that holds one message. The module's Limits section names only consecutive messages.
  - What reaches it: the Chat decoder joins every content delta into one `AssistantText` (`llm-chat/src/incoming.rs:245`). Within one chunk it relays `content` before `reasoning_content` (`incoming.rs:121-140`). That vLLM sends both fields in one chunk is my inference; I did not observe it.
  - Fix to name, inside this unit: do not end the message on a reasoning delta. The cost is that a Responses upstream with message, reasoning, message would then be refused instead.
- **`ingress.rs:214` — the same split on a call (warning, INFEASIBLE here).**
  - What breaks: one Chat message with a call between its deltas is refused at `close`.
  - This conflicts with pass-1's `text_call_text`: the events are identical and the outcomes differ, so without a block boundary at most one of the two can pass.
  - Needs coordinator: route this to story:ingress-block-boundaries and add it to the module's Limits section.
- **`ingress.rs:829` — an extra argument event (note, NEEDS-CHANGE).**
  - What breaks: a call announced with no argument fragments gets a `{"}"`-free `{}` delta added at `close`, so the decoded events differ from the encoded ones. The finished item already carries the arguments.
  - What reaches it: Chat reads empty arguments as `{}` (`incoming.rs:477`).
- **`ingress.rs:465` — empty messages are matched one-sidedly (note, INFEASIBLE).**
  - What breaks: `check_streamed` counts empty carried messages, but `shown_messages` skips empty streamed ones. The outcome `["", call, "Checking."]` is refused although it would read back unchanged.
  - What reaches it: the Messages decoder yields `""` for an empty text block and relays no delta for it. I did not find an upstream that produces this shape.

**5. Attacked and could not break:**
- Usage subsets and total derivation.
- Stop-reason edge values `unspecified` and a 64-byte reason.
- Arguments: interleaved deltas across two calls, empty argument deltas, whitespace-only relays, key order.
- Ordering: output order differing from announcement order, a second reasoning item, the `MAX_ITEMS` boundary on read-back.
- Stream state: `sequence_number` after a refused close, a refused close then `fail`.
- The conformance adapter and all 13 scenarios agree with the code.

**6. Paths written outside the worktree**, both in the assigned scratch dir:
- `~/.cache/llm-wave-w07/responses-ingress-encoder/scratch/adversary-pass2-alone.log`
- `~/.cache/llm-wave-w07/responses-ingress-encoder/scratch/adversary-pass2-suite.log`

I acquired a session lease on the tree (`adversary-w07-responses-ingress-encoder-pass-2`) and released it.

**7. Findings block**
```findings
- file: crates/llm-responses/src/ingress.rs
  line: 397
  category: boundary
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "fail relays a usage snapshot marked final_usage false into response.failed, which the decoder reads back with final_usage true, so partial counters become a final cost"
- file: crates/llm-responses/src/ingress.rs
  line: 186
  category: contract-drift
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "a reasoning delta between two text deltas of one message opens a second message item, so close refuses the single-message outcome the Chat decoder produces; the documented limit names only consecutive messages"
- file: crates/llm-responses/src/ingress.rs
  line: 214
  category: contract-drift
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "one message whose deltas a call interrupts is refused at close; it cannot pass alongside the pass-1 text_call_text case without a block boundary and is absent from the documented limits and story:ingress-block-boundaries"
- file: crates/llm-responses/src/ingress.rs
  line: 829
  category: acceptance
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "a call announced with no argument fragments gains a ToolArgumentsDelta at close, so the decoded events differ from the encoded ones"
- file: crates/llm-responses/src/ingress.rs
  line: 465
  category: boundary
  severity: note
  verdict: needs-revision
  origin: introduced
  message: "check_streamed counts empty carried messages while shown_messages skips empty streamed ones, so an outcome with an empty message before a shown one is refused although it reads back unchanged"
```
