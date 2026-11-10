---
format: aep.planning-md/3
id: review-result:adversary-github-zero-u1-pass-1
kind: review-result
status: active
title: Adversary, pass 1 (story:ingress-block-boundaries, wave 2026-11-09-github-zero)
relations:
- reviews: story:ingress-block-boundaries
revision: 1
---
unit: llm u1 story:ingress-block-boundaries, worktree llm-gh-zero-u1 at 174b1014 plus 2 untracked test files
verdict: CONFIRMED
cases: executed 583→586, red 3
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: 5 paths (part 6)
needs-coordinator: none

**1. Diff of the worktree**

`git --no-pager diff --stat` is empty, because nothing tracked was changed. `git status --short`:
```
?? crates/llm-messages/tests/adversary_w2026_11_09_block_boundaries.rs
?? crates/llm-responses/tests/adversary_w2026_11_09_block_boundaries.rs
```
Both paths are test files. No implementation file was touched.

**2. Cases added.** Each was run alone, before the suite. All three are red now.

`crates/llm-messages/tests/adversary_w2026_11_09_block_boundaries.rs`
- `redacted_thinking_does_not_spend_the_callers_sink_budget`: a Messages answer with an 8 KiB `redacted_thinking` block and the text "Done.", decoded into `VecSink::new(16, 4096)`. That is the bound `crates/llm-core/examples/embedded.rs` uses. The case asserts the turn decodes. It fails:
  ```
  panicked at crates/llm-messages/tests/adversary_w2026_11_09_block_boundaries.rs:92:27:
  a turn showing five bytes of text fits a 4 KiB sink: redacted thinking is not shown: Error { code: TooLarge, message: "stream sink byte bound reached", dispatch: Accepted, ... retriable: false ...
  ```
- `a_message_boundary_takes_no_event_slot_of_the_callers_sink`: text, tool_use, text. That shows 4 events, decoded into `VecSink::new(4, 64 KiB)`. The case asserts the turn decodes. It fails:
  ```
  panicked at crates/llm-messages/tests/adversary_w2026_11_09_block_boundaries.rs:138:13:
  four shown events fit a sink of four: a boundary is not output: Error { code: TooLarge, message: "stream sink event bound reached", dispatch: Accepted, ... retriable: false ...
  ```

`crates/llm-responses/tests/adversary_w2026_11_09_block_boundaries.rs`
- `text_after_a_boundary_never_continues_a_message_shown_before_it`: Responses deltas arrive as `msg_a "Al"`, `msg_b "Beta."`, `msg_a "pha."`, and the turn completes with `[Alpha., Beta.]`. The case asserts that the text runs between boundaries are the turn's messages. It fails:
  ```
  panicked at crates/llm-responses/tests/adversary_w2026_11_09_block_boundaries.rs:83:5:
  each run of text between boundaries is one of the turn's messages: [TextDelta { text: "Al" }, MessageBoundary, TextDelta { text: "Beta." }, MessageBoundary, TextDelta { text: "pha." }]
    left: ["Al", "Beta.", "pha."]
   right: ["Alpha.", "Beta."]
  ```

**3. Suite run** (after the cases existed)

`RUSTUP_TOOLCHAIN=1.98.0 b10x-build-slot cargo test -p llm-messages -p llm-responses --no-fail-fast` exited with 101.
- 583 passed, 3 failed, 0 ignored.
- The only failing targets are the two `adversary_w2026_11_09_block_boundaries` binaries.
- The before count of 583 comes from the same run with those two binaries left out of the total.
- The full log is in the scratch directory as `suite.log`.

**4. Findings** (they cover 174b1014)

| file:line | verdict | origin | what was measured | what reaches it |
|---|---|---|---|---|
| crates/llm-messages/src/decode.rs:486 | CONFIRMED | introduced | The `OpaqueItem` for `redacted_thinking` copies the whole block into the sink. `VecSink` charges it against the byte bound, so a turn showing 5 bytes of text fails `too-large` and is final (test:92). At base the block emitted nothing (`git show c21ba4b2` decode.rs:478). | Any `VecSink` caller, with a Messages upstream that sends redacted thinking larger than the remaining budget. The examples size the sink at 4 KiB (embedded.rs) and 64 KiB (local_endpoint, live_subscription_turn). Bedrock InvokeModel goes through the same path (`invoke.rs:382`). |
| crates/llm-core/src/port.rs:75 | CONFIRMED | introduced | `MessageBoundary` is documented as carrying nothing and not being output, yet it takes a `VecSink` event slot. Text, call, text no longer fits a sink bounded at 4 (test:138). | Any `VecSink` caller whose event bound the answer reaches exactly. Each later message adds one event. |
| crates/llm-core/src/port.rs:64 | INFEASIBLE | introduced | The promise "never continues one shown before it" is broken: the Responses decoder (`stream.rs:358`) emits a boundary on every change of item, so interleaved deltas give 3 runs of text for 2 messages (test:83). | Nothing found. No Responses server is known to interleave message deltas. The decoder does accept out-of-order deltas, and the contract scenario `kept-text-a-second-message-shown-only-as-deltas-is-kept` has them. |

Fix to name for rows 1–2: either `VecSink` leaves `MessageBoundary` and `OpaqueItem` out of its bounds, or the docs say those bounds now count both. Rows 1–2 are the same defect, so one fix covers both. For row 3, either narrow the doc sentence, or have the decoder refuse or warn on interleaved items.

**5. Attacked and not broken**
- **Fallback:** a boundary is never the first event in Messages, Responses, Bedrock Converse or InvokeModel, because each decoder emits it only after text it has shown. `Counting` counts `OpaqueItem`, so no fallback happens after one, and no opaque item crosses a binding.
- **Timed:** the first-output clock skips the boundary and starts on `OpaqueItem`.
- **Messages relay:**
  - A boundary at the start, at the end, twice, after a tool, or after thinking does nothing wrong (`end_text` only stops an open text block).
  - Thinking, redacted thinking, text and redacted thinking, thinking, text both keep their order.
  - `whole` checks the item against the event's own provenance, not the binding. No producer emits an item with a foreign provenance, so I wrote no case for it.
- **Responses relay:** the boundary only clears `in_message`. Chat-shaped text, call, text stays one message item.
- **Chat relay:** it drops both new events, and `close` refuses opaque state.
- **Serde:** `deny_unknown_fields` holds on both variants, and `Provenance` denies unknown fields. I ran no serde case.
- **Bedrock:** streamed blocks and whole answers emit the boundary the same way, and empty text emits none.

**6. Paths written outside the worktree** (all under the unit's assigned `scratch-adv/` directory)
- `messages-case.log`: this first run failed only because I used the wrong package name.
- `messages-case2.log`
- `responses-case.log`
- `suite.log`
- `suite-summary.txt`

The build output went to the worktree's own `target/`.

**7. Findings block**

```findings
- file: crates/llm-messages/src/decode.rs
  line: 486
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a redacted_thinking block now enters the caller's sink as an OpaqueItem carrying its whole payload, so a VecSink byte bound sized for shown text refuses the turn too-large and final where it succeeded before
- file: crates/llm-core/src/port.rs
  line: 75
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: MessageBoundary is documented as carrying nothing and not being output, yet VecSink counts it against its event bound, so an answer that exactly fit the sink now fails too-large
- file: crates/llm-core/src/port.rs
  line: 64
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the promise that text after a boundary never continues an earlier message fails on interleaved Responses item deltas, which the decoder accepts and splits into three text runs for two messages
```
