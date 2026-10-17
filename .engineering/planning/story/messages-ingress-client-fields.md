---
format: aep.planning-md/3
id: story:messages-ingress-client-fields
kind: story
status: implemented
title: Messages ingress and egress hold Claude Code's first turn under written rules
summary: thinking, system blocks, cache_control, effort max, disable_parallel_tool_use, metadata
relations:
- decomposes: epic:client-request-fields
- serves: vision:portable-model-inference
- depends_on: story:neutral-turn-controls
scope:
- confidence: cited
  path: checks/conformance/src/messages.rs
- confidence: cited
  path: contracts/messages/scenarios
- confidence: cited
  path: crates/llm-messages/src
- confidence: cited
  path: crates/llm-messages/tests
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: spec/domains/messages.yaml
revision: 13
transitions:
- {from: "draft", to: "proposed", at: "2026-10-17T04:25:33Z", actor: "human:timo", revision: 11, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "proposed", to: "active", at: "2026-10-17T04:25:34Z", actor: "human:timo", revision: 12, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "active", to: "implemented", at: "2026-10-17T07:23:40Z", actor: "human:timo", revision: 13, decided_on: {"recorded":{"test_result":1,"review_outcome":4}}}
---
## Context

`llm_messages::decode_request` refuses Claude Code's first turn field after field: `thinking`, a
`system` of more than one block, `cache_control` on a content block, and effort `max`, which no
capability names (`crates/llm-messages/src/codec.rs` `decode_request`, `instructions`,
`decode_block`; the authored fixture `beyond10x/llm-gateway`
`crates/llm-gateway/tests/fixtures/claude-code/1-tool-turn.request.txt`).

## Design

`docs/design.md` "Client request fields", Messages column and the Messages ingress rule:

- Ingress: `thinking` (`type` `adaptive`, `enabled` with `budget_tokens`, `disabled`; any other
  key, `display` included, refused by its path) → `controls.thinking`. A `system` string, or an
  array of text blocks joined by `\n`, → instructions; a non-text block refused. `cache_control`
  `{"type":"ephemeral"}` with an optional `ttl` of `5m` or `1h`, on a system block, a `text`,
  `tool_use` or `tool_result` block, or a tool → `hints.prompt_cache`; any other shape refused by
  path; two different lifetimes in one request refused; never on a `thinking` or
  `redacted_thinking` block. `output_config.effort` takes any non-empty name. 
  `tool_choice.disable_parallel_tool_use` (boolean) → `controls.parallel_tool_calls = !v`.
  `metadata` (an object of `user_id` only) → `hints.unforwarded` `client-metadata`; its value is
  never kept. Everything else stays refused by its path.
- Egress (`encode_request`): `thinking` re-emitted as read; `parallel_tool_calls` as
  `tool_choice.disable_parallel_tool_use` (writing `{"type":"auto"}` when the choice is
  automatic); a `1h` prompt-cache lifetime on every breakpoint the projection places; effort
  through `llm_core::wire_effort`.
- `omitted_advisories(&TurnRequest)` exported: `prompt-cache-key`, `verbosity`,
  `reasoning-summary` and the unforwarded names, whichever are present. `MessagesClient` emits
  `llm_core::advisory_warning` for them after a successful turn.

ESS first: `spec/domains/messages.yaml` gains the observed `omitted_advisories` and the rule text.

## Shared files

Besides `crates/llm-messages`: `spec/domains/messages.yaml`, `checks/conformance/src/messages.rs`,
`contracts/messages/scenarios/`, `docs/messages.md`. The coordinator owns
`contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/schema/` and
`contracts/baseline.json`: the branch lists its scenarios there and regenerates the projections
only in an uncommitted working copy to run its own check, and the coordinator lists them and
regenerates once on the merged wave.

## Acceptance

- The fixture's body, copied to `crates/llm-messages/tests/fixtures/claude-code-first-turn.json` with
  its provenance in a `README.md` beside it, decodes: instructions
  are its two system texts joined by `\n`, `thinking` adaptive, effort `max`, `prompt-cache`
  without a lifetime, one user text, one tool.
- Round trips, each decoded, re-projected with `encode_request` and decoded back to an equal turn:
  `thinking` in each of its three forms, `disable_parallel_tool_use` true and false, a `1h`
  prompt-cache lifetime, effort `max`. The one-way rules are asserted as one-way: the joined
  `system` comes back as one block, `metadata` is not re-emitted and reported omitted.
- Refused by path: `thinking.display`, `context_management`, `safeguards`,
  `output_config.format`, `system[].citations`, `messages[].content[].cache_control.scope`,
  `cache_control` on a `thinking` block, two lifetimes; a `system` message inside `messages` is
  refused as a role outside the subset.
- `omitted_advisories` and the client's single trailing `advisory-omitted` warning, absent when
  nothing is omitted and absent from a failed turn.
- Conformance scenarios in `contracts/messages/scenarios/` for the decoded fixture, each round
  trip and each refusal, passing on the merged wave with none skipped.

## Verification

`cargo test -p b10x-llm-messages`; `cargo clippy -p b10x-llm-messages --all-targets -- -D
warnings`; `task conformance` on the merged wave.
