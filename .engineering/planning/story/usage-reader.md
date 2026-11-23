---
format: aep.planning-md/3
id: story:usage-reader
kind: story
status: active
title: A public usage reader per protocol reads model and counters from an answer's bytes
summary: Gateway asks for a usage reader per wire, for whole bodies and streams fed in pieces (llm-gateway dependency-blocker:llm-usage-decoders)
refs:
- provider: github
  reference: beyond10x/llm-gateway/.engineering/planning/dependency-blocker/llm-usage-decoders.md
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-23T01:05:59Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-23T01:05:59Z", actor: "human:timo", revision: 3}
---
## Context

LLM Gateway cannot record two fields the provider reports: the model the answer names and the
reasoning-token count. Its relay reads four token counts with a scanner of its own
(`crates/llm-gateway/src/usage.rs`, fed from `relay/passthrough.rs` at `de2248a7`, line 470) and
writes `reported_model` and `reasoning_output_tokens` as absent (`server.rs` line 651). Its open
requirement `dependency-blocker:llm-usage-decoders` (beyond10x/llm-gateway, revision 2) asks llm
for a public usage reader for each of the three HTTP protocols, for answers that are not streamed
and for streamed answers read piece by piece.

At llm 0.24.0 (`08289a72`) the Responses decoder reads both fields
(`crates/llm-responses/src/stream.rs` lines 94 and 1024), but its public entry point,
`decode_stream`, takes a complete list of decoded payloads and needs a binding. The Messages usage
snapshot (`usage.rs`) and the Chat Completions report reader (`incoming.rs`, `read_reported`) are
private. Review finding 1 of 2026-11-23 on 0.24.0.

llm owns the reader. The gateway owns recording what it returns.

## Outcome

Each protocol crate exports one usage reader that takes an answer body as bytes, in pieces split
anywhere, and returns one neutral value: the model the answer names and the five counters (input,
output, cache read, cache write, reasoning), each unknown unless the answer reported it. The
reader uses the decoder's own usage parsing, so the two cannot drift apart.

## Acceptance

- `llm_core::ReportedUsage` holds `model: Option<Id>`, `usage: Usage` and `final_usage: bool`.
- `llm_responses::UsageReader`, `llm_messages::UsageReader` and `llm_chat::UsageReader` each have
  `new`, `feed(&mut self, &[u8])` and `finish(self) -> ReportedUsage`, and each crate has
  `read_usage(&[u8]) -> ReportedUsage` for a whole body. A body whose first byte that is not white
  space is `{` is read as one JSON answer; any other body as a server-sent-event stream.
- On the recorded Codex capture (`crates/llm-responses/tests/fixtures/live/codex-text-turn.sse`)
  the Responses reader reports model `gpt-6-astra`, input 34, output 80, cache read 0, reasoning
  50, and exactly the counters and model `decode_stream` reports for the same capture.
- On both recorded Messages captures the Messages reader reports the model and the counters
  `decode_stream` reports for the same capture.
- A Chat Completions stream with a usage chunk and a non-streamed Chat Completions body report the
  model and every counter they carry, reasoning included, and every pinned Chat Completions stream
  reports the model and counters `project_response_bytes` reports; the body those
  `decode_completion` reports.
- A non-streamed Responses body and a non-streamed Messages body report their model and counters.
- Every counter an answer did not report is `None`, never 0, on all three protocols.
- Fed whole, split into two pieces, and fed one byte at a time, each fixture gives the same
  `ReportedUsage`. Every split point is checked for every JSON body, both Messages captures,
  `openai-text-and-usage.sse` and `openai-partial-usage.sse`. The 15 KB Codex capture is checked at
  every 61st split and at every split within 128 bytes of its usage report, and the other pinned
  Chat Completions streams at every 17th split: every split of all of them took 121 s and 34 s in
  a debug build, past the 60 s test budget (one split is one whole reading).
- A stream that stops before its terminal event reports what it carried so far with
  `final_usage` false; a terminal event, or a whole JSON body, sets it true.
- The new tests are named in AGENTS.md's "Unknown usage stays unknown" row; no test makes a network
  request or a model call; the reader tests of each crate run well under 60 s.

## Design decisions

- The output type lives in `llm-core` beside `Usage`: it is a neutral value with no transport, so
  the dependency boundary (`llm-core` has no transport dependency) holds.
- The byte layer (JSON or event stream, chosen by the first byte that is not white space, framed
  by `SseDecoder`) is shared in `llm-http` as `AnswerBody`, which all three protocol crates
  already depend on.
- Counters follow the neutral `Usage` meaning, the same as the turn decoders: input includes cache
  read and cache write. On Messages the input total is therefore known only when the answer
  reported all three of its parts, as `Snapshot::normalized` already decides.
- A report a decoder would refuse (a counter that is not a whole number, contradictory subsets) is
  not taken: the reader keeps what it had before it, so a bad report never turns into a number.
- The reader never fails: it returns what it read. A framing failure stops reading at that point.
