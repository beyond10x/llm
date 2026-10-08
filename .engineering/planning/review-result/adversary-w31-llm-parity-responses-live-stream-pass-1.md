---
format: aep.planning-md/3
id: review-result:adversary-w31-llm-parity-responses-live-stream-pass-1
kind: review-result
status: archived
title: Wave 2026-10-05-w31 adversary, llm story:parity-responses-live-stream, pass 1
relations:
- reviews: story:parity-responses-live-stream
revision: 2
transitions:
- {from: "active", to: "archived", at: "2026-10-08T10:43:38Z", actor: "human:timo", revision: 2}
---
```
unit: llm/parity-responses-live-stream, working tree at 09f60a5 plus one untracked test file
verdict: red
cases: executed 666→670, red 4
origin: introduced 4 / pre-existing 2 / undecided 0
wrote-outside-worktree: ~/.cache/ga-wave-2026-10-05-w31/llm-parity-responses-live-stream/scratch/{adversary1-red.log, adversary1-suite.log}
needs-coordinator: yes (kept text per message; keepalive as answered)
```

Cases added: `crates/llm-responses/tests/adversary_w31_live.rs` (client and decode_stream agree past the terminal object; a second message shown only as deltas is kept; kept messages ordered by output_index; a cut after only a keepalive stays retriable). Suite: 666 passed, 4 failed.

| # | file:line | finding | verdict / origin |
|---|---|---|---|
| 1 | crates/llm-responses/src/stream.rs:74 | decode_stream keeps decoding after the terminal object while the client stops | CONFIRMED / introduced, note |
| 2 | crates/llm-responses/src/stream.rs:271 | any assistant item disables kept text, dropping a second message shown only as deltas | CONFIRMED / introduced, warning |
| 3 | crates/llm-responses/src/stream.rs:281 | kept messages inserted in first-delta order, not output_index order | CONFIRMED / introduced, note |
| 4 | crates/llm-responses/src/client.rs:174 | a keepalive marks the turn answered; Harness retries | CONFIRMED / pre-existing, warning |
| 5 | crates/llm-responses/tests/adversary_w27_retry.rs:4 | module doc of the w27 file rewritten | CONFIRMED / introduced, note |
| 6 | crates/llm-responses/tests/adversary_codex_stream.rs:353 | the R36 test asserts nothing when decoding refuses | CONFIRMED / pre-existing, note |

Not broken: event order; bounds mid-stream; sink refusal and cancel (usage absent, not zero); refusal after the terminal keeps counters; reasoning and tool-argument deltas never kept as text; no kept text for failed/error/refusal; huge, negative or missing output_index; Codex untyped 200 with empty output; routing and client agree on answered.

```findings
[
{"file":"crates/llm-responses/src/stream.rs","line":74,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"decode_stream keeps decoding after the terminal object while the client stops, so its events and kept text differ from what the live caller gets, against the new doc and spec claim of equality"},
{"file":"crates/llm-responses/src/stream.rs","line":271,"category":"acceptance","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"any streamed assistant item disables kept text, so a second message shown only as deltas is dropped although the terminal output is empty"},
{"file":"crates/llm-responses/src/stream.rs","line":281,"category":"boundary","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"kept messages are inserted in first-delta order, so out-of-order first deltas place a message after a call that has a higher output_index"},
{"file":"crates/llm-responses/src/client.rs","line":174,"category":"judgement","severity":"warning","verdict":"CONFIRMED","origin":"pre-existing","message":"a keepalive marks the turn answered, so a cut after only lifecycle and keepalive payloads is final where Harness would retry"},
{"file":"crates/llm-responses/tests/adversary_w27_retry.rs","line":4,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"the w27 adversary file the brief required unedited had its module doc rewritten"},
{"file":"crates/llm-responses/tests/adversary_codex_stream.rs","line":353,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"pre-existing","message":"the R36 named test wraps its assertion in if-let-Ok and passes without asserting when decoding refuses"}
]
```
