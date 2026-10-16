---
format: aep.planning-md/3
id: review-result:adversary-w07-messages-ingress-encoder-pass-2
kind: review-result
status: active
title: Wave 2026-10-16-w07 adversary, story:messages-ingress-encoder, pass 2
relations:
- reviews: story:messages-ingress-encoder
revision: 1
---
```
unit: story:messages-ingress-encoder (tree llm-w07-messages-ingress-encoder, head b2343ef2 plus one untracked test file)
verdict: red
cases: executed 129→133, red 1
origin: introduced 1, pre-existing 0, undecided 0
wrote-outside-worktree: ~/.cache/llm-wave-w07/messages-ingress-encoder/scratch/adversary-pass2/{case-run.log,suite-run.log,clippy.log}
needs-coordinator: yes. The finding is a design fork: should the encoder relay reasoning before it knows the outcome can sign it?
```

The one red case shows that a Messages caller served from a Chat upstream with a reasoning model gets an error at the end of every turn, after the whole answer was already streamed. The three pass-1 corrections hold.

**1. Diff.** `git diff --stat` is empty. The only change is one untracked test file, `crates/llm-messages/tests/adversary_w07_ingress_pass2.rs`. No implementation file was touched.

**2. Cases added.** I wrote all four first, then ran that file alone (exit 101, 3 passed, 1 failed):

| test | what it asserts | now |
|---|---|---|
| `a_local_reasoning_turn_relayed_from_chat_is_answered_and_reads_back` | events [reasoning "The user greets me.", text "Guten Tag"] with an outcome holding only that text close, and read back equal | red: `close refused it after streaming the whole answer: Error { code: Protocol, message: "Messages answer contradicts what was already streamed", dispatch: NotSent, … }` at :140 (now :144 after rustfmt) |
| `interleaved_thinking_and_text_read_back_whole` | reasoning, text, reasoning, text: two thinking blocks open at once, both read back | green |
| `a_call_stopped_before_any_fragment_reads_back_only_as_empty_arguments` | a call stopped before any fragment reads back with `{}`; any other arguments are refused | green |
| `text_resembling_framing_and_astral_characters_reads_back_exactly` | text containing SSE-like framing, CRLF, NUL, U+2028, emoji and ZWJ, split across deltas, reads back byte for byte | green |

**3. Suite run** (after the cases existed): `RUSTUP_TOOLCHAIN=1.98.0 cargo test -p b10x-llm-messages --locked --no-fail-fast` exited 101, with 132 passed and 1 failed out of 133. All other binaries are `ok`, including `adversary_w07_ingress: 5 passed` and `ingress: 21 passed`. The 129 before-count is that total minus my 4 cases. I ran rustfmt on my file (line breaks only), and `cargo fmt --all --check` and clippy `-D warnings` now both exit 0.

**4. Finding** (covers b2343ef2):
- **`crates/llm-messages/src/ingress.rs:271`: reasoning that can never be signed fails the whole turn.**
  - What happens: `reasoning()` opens a `thinking` block on the first `ReasoningDelta`. Only an opaque block in the outcome can sign it. When the outcome has none, `close` refuses (:180-188). The outcome itself has a valid Messages answer (one text block), and the story's Acceptance requires the decoded outcome to equal it.
  - What reaches it: the Chat decoder sends vLLM `reasoning_content` as `ReasoningDelta` only, never as an item (`crates/llm-chat/src/incoming.rs:131-139`). `crates/llm-chat/tests/local_endpoint.rs:95-105` measures exactly these strings, with `items == [assistant("Guten Tag")]`. So any gateway relaying that stream through the public `IngressStream` API hits this.
  - Conflict: this contradicts the unit's own first assertion in `tests/ingress.rs:485` (`reasoning_without_a_signature_is_refused`), so a fix means a design call.
  - Fix options, not applied: an option at `new` not to relay reasoning, or a documented rule that the server filters out `ReasoningDelta` when its upstream cannot sign it.
  - Status: CONFIRMED, introduced.

**5. Attacked and could not break:**
- **Held calls:** pass-1's held-call correction holds for both ways a call gets held. The two re-pins (model, redacted thinking) match the limits already decided and are not relaxed.
- **Content:** interleaved thinking, closed empty-argument calls, framing-like text and unicode all held.
- **Usage:** a known input total is left out whenever cache creation is unknown. That is every Chat and Responses upstream (`llm-responses/src/stream.rs:701`). The unit pins this in `tests/ingress.rs:325`, the wire cannot carry it honestly, and the module's list of limits doesn't mention it.
- **`fail`:** `protocol`, `cancelled` and `unsupported` read back as `transport` or `invalid_request`. The Acceptance line says "the error's code", but no Messages error type maps back to those three. This is documented at `ingress.rs:220-224`.
- **Thinking with empty text:** a relayed upstream thinking block with empty text hits the same no-neutral-event limit as redacted thinking (story:ingress-block-boundaries).

**6. Paths written outside the worktree:** the three logs in the header. I acquired and released my lease `adversary-w07-messages-ingress-encoder-pass-2`.

```findings
- file: crates/llm-messages/src/ingress.rs
  line: 271
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "a Chat-upstream reasoning turn (ReasoningDelta streamed, outcome holding only text, as llm-chat/tests/local_endpoint.rs measures) has a faithful one-text-block answer, but IngressStream opens a thinking block it can never sign and close refuses the turn after streaming it (adversary_w07_ingress_pass2::a_local_reasoning_turn_relayed_from_chat_is_answered_and_reads_back)"
```
