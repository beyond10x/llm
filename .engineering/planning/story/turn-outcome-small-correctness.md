---
format: aep.planning-md/3
id: story:turn-outcome-small-correctness
kind: story
status: implemented
title: A returned turn reports its cache writes, its call count and its output-limit stop
summary: 'Review of 0.24.0, findings 4, 7, 8: Responses cache-write count, parallel_tool_calls=false unchecked, llm infer hides max_output_tokens'
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-23T13:37:17Z", actor: "human:timo", revision: 2, decided_on: {"recorded":{"test_result":1}}}
- {from: "proposed", to: "active", at: "2026-11-23T13:37:18Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"test_result":1}}}
- {from: "active", to: "implemented", at: "2026-11-23T13:37:19Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}}
---
## Context

Review of llm 0.24.0 (2026-11-23, `LLM-findings.txt`), three small correctness findings on what a
returned turn says:

- Finding 8: Responses discards a reported cache-write count. `stream::usage` sets
  `cache_creation_input_tokens` to `None` whatever the answer carries, so a missing count, an
  explicit `0` (the recorded Codex turn reports `input_tokens_details.cache_write_tokens: 0`) and a
  positive count all read as unknown. The crate's own ingress encoder writes a positive count as
  `input_tokens_details.cache_creation_input_tokens`, and the decoder loses it on readback. A priced
  `llm infer` route on `responses` therefore stays `cost=unknown` even when every counter was
  reported. `UsageReader` (0.25.0) reads through the same function.
- Finding 7: `controls.parallel_tool_calls = Some(false)` is sent (Responses and Chat
  `parallel_tool_calls: false`, Messages `disable_parallel_tool_use: true`) but
  `TurnOutcome::validate_for` never checks the count of returned calls, so a turn proposing two
  calls is returned as a success.
- Finding 4: `llm infer` hides an output-limit stop: a completed turn and one stopped at
  `max_output_tokens` give identical stdout, stderr and exit status 0.

## Acceptance

1. A Responses answer whose `usage.input_tokens_details` carries `cache_write_tokens` or
   `cache_creation_input_tokens` decodes that count, `0` included, as
   `Usage::cache_creation_input_tokens`; an answer carrying neither keeps it `None`; two spellings
   that disagree keep it `None` (finding 8).
2. `llm_responses::UsageReader` reports the same cache-write count as the turn decoder (finding 8).
3. A priced `llm infer` route on `responses` whose answer names the route's model and reports every
   counter prints a known `cost`, not `cost=unknown` (finding 8).
4. A returned turn with more than one tool call is refused as `Protocol` when the request's
   `controls.parallel_tool_calls` is `Some(false)`, on every route that validates an outcome; one
   call, `Some(true)` and `None` are accepted as before (finding 7).
5. `llm infer` on a turn that stopped at its output limit keeps the partial answer on stdout, says
   on stderr that the turn stopped at the output limit, and exits with the status the guide states
   for it; a turn that ended for another incomplete reason says that reason (finding 4).
