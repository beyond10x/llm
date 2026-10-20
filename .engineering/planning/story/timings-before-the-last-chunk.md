---
format: aep.planning-md/3
id: story:timings-before-the-last-chunk
kind: story
status: draft
title: A llama-server timings before the last chunk is not the turn's count
summary: Read timings only from the report that ends a stream
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 2
---
## Context

Reported on 2026-10-21 by the `inference` repository's adversary, relayed by session
`b10x-chef-orchestrator`. Found on offline streams; not observed against a live llama-server, and
inference does not know whether llama-server `b11752` writes either shape. Its recorded usage chunk
carries `timings`, as `crates/llm-chat/fixtures/llama-server-text-and-usage.sse` does.

At tag 0.8.3 `read_reported` (`crates/llm-chat/src/incoming.rs:437-461`) keeps the latest
`timings` from any chunk, and `Reported::counters` (`incoming.rs:52-69`) fills every counter
`usage` leaves unknown from it. `agree` (`incoming.rs:542-564`) holds a new `usage` against a
`timings` kept from an earlier chunk. So a `timings` written before the last chunk is read as the
turn's report:

1. `timings` only on a chunk before the last (`predicted_n` 2) and no usage chunk after it: the
   turn reports output 2 as provider-reported with `final_usage` true, while the server produced
   4. On 0.8.0, which did not read `timings`, the same stream reported usage unknown.
2. `timings` mid-stream (`predicted_n` 2), then a usage chunk without `timings`
   (`completion_tokens` 4): the turn fails as "chat completion timings contradict its usage
   report", and the error's observation keeps usage {input 35, output 2, cached 0}.

Open before implementing: whether llama.cpp `7879491cf` writes `timings` on a chunk before the
last (for example under a per-token timings request option). Read it from
`tools/server/server-context.cpp` and `server-common.cpp` and cite the line; the fix holds either
way, because a count taken from a chunk that does not end the stream is not the turn's count.

## Fixtures

Both are written in the shape of the existing llama-server fixtures; the token text is invented.
The server produced 4 output tokens in each (`Hel`, `lo`, ` there`, `.`).

`crates/llm-chat/fixtures/llama-server-timings-before-the-last-chunk.sse`:

```text
data: {"choices":[{"finish_reason":null,"index":0,"delta":{"role":"assistant","content":null}}],"created":1792600000,"id":"chatcmpl-timingsmidstream0000000000000001","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":"Hel"}}],"created":1792600000,"id":"chatcmpl-timingsmidstream0000000000000001","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":"lo"}}],"created":1792600000,"id":"chatcmpl-timingsmidstream0000000000000001","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk","timings":{"cache_n":0,"prompt_n":35,"prompt_ms":301.5,"prompt_per_token_ms":8.614285714285714,"prompt_per_second":116.08623548922056,"predicted_n":2,"predicted_ms":140.2,"predicted_per_token_ms":70.1,"predicted_per_second":14.265335235378032}}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":" there"}}],"created":1792600001,"id":"chatcmpl-timingsmidstream0000000000000001","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":"."}}],"created":1792600001,"id":"chatcmpl-timingsmidstream0000000000000001","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":"stop","index":0,"delta":{}}],"created":1792600001,"id":"chatcmpl-timingsmidstream0000000000000001","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: [DONE]
```

`crates/llm-chat/fixtures/llama-server-timings-before-a-usage-chunk.sse`:

```text
data: {"choices":[{"finish_reason":null,"index":0,"delta":{"role":"assistant","content":null}}],"created":1792600100,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":"Hel"}}],"created":1792600100,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":"lo"}}],"created":1792600100,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk","timings":{"cache_n":0,"prompt_n":35,"prompt_ms":301.5,"prompt_per_token_ms":8.614285714285714,"prompt_per_second":116.08623548922056,"predicted_n":2,"predicted_ms":140.2,"predicted_per_token_ms":70.1,"predicted_per_second":14.265335235378032}}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":" there"}}],"created":1792600101,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":null,"index":0,"delta":{"content":"."}}],"created":1792600101,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[{"finish_reason":"stop","index":0,"delta":{}}],"created":1792600101,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk"}

data: {"choices":[],"created":1792600101,"id":"chatcmpl-timingsmidstream0000000000000002","model":"qwen3-1.7b","system_fingerprint":"b11752-7879491cf","object":"chat.completion.chunk","usage":{"completion_tokens":4,"prompt_tokens":35,"total_tokens":39,"prompt_tokens_details":{"cached_tokens":0}}}

data: [DONE]
```

## Outcome

A llama-server stream's usage is read from the report that ends it: the chunk carrying the finish
reason or the usage chunk after it. A `timings` on an earlier chunk is held to its shape and is
neither the turn's count nor evidence against a later `usage`.

## Acceptance

- `llama-server-timings-before-the-last-chunk.sse` ends with usage unknown, as on 0.8.0, and never
  output 2 with `final_usage` true.
- `llama-server-timings-before-a-usage-chunk.sse` ends without error with usage {input 35, output
  4, cached 0} from the usage chunk.
- A `timings` on an earlier chunk in the wrong shape is still refused by its field name.
- `usage` and `timings` on the same ending chunk that disagree are still refused, and the three
  existing llama-server fixtures and their five ESS scenarios still pass unchanged.
- Both fixtures run as ESS conformance scenarios; the suite baseline moves from 888 to 890.

## Scope

- `crates/llm-chat/src/incoming.rs`: `Reported`, `read_reported`, and the stream path that feeds
  it (`StreamProjection::accept`, `finish`); the non-streamed `decode_completion` is one report
  and stays as it is.
- `crates/llm-chat/fixtures/`: the two fixtures above.
- `crates/llm-chat/tests/incoming.rs`: one test per acceptance line.
- `contracts/ess-inputs.yaml`: two scenarios beside the five `llama-server-*` ones; `contracts/suite.json` regenerated with ess 0.66.0.

Size S: one decoder function and its state, two fixtures, five tests, two scenarios.
