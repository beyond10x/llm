---
format: aep.planning-md/3
id: story:client-turns-cross-wire
kind: story
status: implemented
title: A Claude Code and a Codex first turn cross to the other two wires
summary: Cross-wire acceptance over the three protocol crates
relations:
- decomposes: epic:client-request-fields
- serves: vision:portable-model-inference
- depends_on: story:messages-ingress-client-fields
- depends_on: story:responses-ingress-client-fields
- depends_on: story:chat-ingress-closed-levels
scope:
- confidence: inferred
  path: checks/conformance/Cargo.toml
- confidence: cited
  path: checks/conformance/tests/client_turns.rs
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-17T04:25:36Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "proposed", to: "active", at: "2026-10-17T04:25:37Z", actor: "human:timo", revision: 7, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "active", to: "implemented", at: "2026-10-17T07:23:41Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2}}}
---
## Context

Each wire story proves its own decoder and projection. Whether a client's real request crosses
to the other two wires needs all three, and only `checks/conformance` depends on all three
protocol crates (`checks/conformance/Cargo.toml`).

## Design

`checks/conformance/tests/client_turns.rs`, run by `cargo test -p b10x-llm-conformance` and so by
`task rust`. It reads the two fixtures from where their stories put them,
`crates/llm-messages/tests/fixtures/claude-code-first-turn.json` and
`crates/llm-responses/tests/fixtures/codex/first-turn.json`, by `include_str!` relative to the
test file. Bindings are built through `llm_routing::Catalog` with capabilities that list `xhigh`
and `high`.

## Acceptance

- Claude Code to Chat: `llm_messages::decode_request` (its `.request`) then `llm_chat::project_request` gives a body
  whose `messages[0]` is `{"role":"system","content":<the two system texts joined by \n>}`,
  whose `reasoning_effort` is `xhigh`, which has no key naming thinking, and whose tool is
  `Read`; `llm_chat::omitted_advisories` is exactly `[prompt-cache]`.
- Claude Code to Responses: the same turn through `llm_responses::project_request` gives
  `input[0]` a developer message with that joined text, `reasoning` `{"effort":"xhigh"}` and the
  `Read` function tool; `llm_responses::omitted_advisories` is exactly `[prompt-cache]`.
- Codex to Chat: `llm_responses::ingest_request` then `llm_chat::project_request` gives
  `parallel_tool_calls` `true`, `prompt_cache_key` equal to the capture's, `reasoning_effort`
  `high`, `messages[0]` the system message holding `instructions` and the developer parts, and four
  function tools; `omitted_advisories` is exactly `[reasoning-summary, input-item-ids,
  client-metadata]`.
- Codex to Messages: the same turn through `llm_messages::encode_request` gives
  `tool_choice` `{"type":"auto","disable_parallel_tool_use":false}`, `output_config.effort`
  `high`, the joined instructions as the marked system block, and four tools;
  `omitted_advisories` is exactly `[prompt-cache-key, reasoning-summary, input-item-ids,
  client-metadata]`.

Every `omitted_advisories` list is in `Advisory` order, the order of the design's advisory table.
Each bullet asserts the named body values, so dropping any of them from a projection fails it.

## Verification

`cargo test -p b10x-llm-conformance --test client_turns`.
