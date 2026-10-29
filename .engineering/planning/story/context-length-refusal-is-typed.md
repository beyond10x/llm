---
format: aep.planning-md/3
id: story:context-length-refusal-is-typed
kind: story
status: active
title: A context-length refusal is told apart from other 400s
summary: A 400 whose structured error names the context window maps to a typed refusal, copying no body text
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-core/src/error.rs
- confidence: cited
  path: crates/llm-http/src/transport.rs
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-28T19:15:33Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-29T20:40:35Z", actor: "human:timo", revision: 3}
---
## Context

`status_error` (`crates/llm-http/src/transport.rs`, 0.16.0) maps every 400 to `Refused` with the
message `HTTP endpoint returned status 400`, and by design copies no error-body text into a
diagnostic. A request refused because it no longer fits the model's context window is therefore
indistinguishable from any other refusal.

Seen from Harness on 2026-10-26: in a one-hour trial on an openai-responses endpoint with a
128,000-token declared window, 7 of 41 runs ended without a terminal record; the three transcripts
read ended right after a `conversation-compacted` warning at 109,063, 125,439 and 120,978 tokens.
Harness 0.21.0 now ends such a run with `finished{stop: failed, cause: provider-refused, message,
after_compaction}`, but `message` can only say `status 400`. Whether those requests were refused
for context length is inferred, not observed: the transcripts were not kept and no stderr was
captured.

## Outcome

A provider refusal whose structured error names the context window (OpenAI Responses
`error.code: "context_length_exceeded"`; the Messages wire's equivalent) reaches the caller as a
distinct, typed refusal, without copying arbitrary body text: a closed set of recognised codes
mapped to a closed set of named causes.

## Acceptance

1. A loopback 400 whose JSON body carries the Responses context-length code yields an error the
   caller can tell apart from any other 400 (a distinct code or a typed reason), and its message
   names the cause in fixed text.
2. A 400 with any other body, or none, maps exactly as today, and no body text appears in any
   diagnostic.

## Out of scope

- Harness compacting and retrying on such a refusal (Harness's store).
