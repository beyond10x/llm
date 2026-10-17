---
format: aep.planning-md/3
id: epic:client-request-fields
kind: epic
status: implemented
title: Every field a coding-agent client sends has one written rule
summary: Ingress carries, translates, reports or refuses each client field by name; nothing is dropped
relations:
- serves: vision:portable-model-inference
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-17T04:25:38Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"review_outcome":1}}}
- {from: "proposed", to: "active", at: "2026-10-17T04:25:38Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"review_outcome":1}}}
- {from: "active", to: "implemented", at: "2026-10-17T07:23:42Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"review_outcome":1}}}
---
## Outcome

A coding-agent client's request crosses wires through the ingress decoders and the egress
projections with every field it carries under exactly one written rule: carried in the neutral
turn, translated by a table, advisory with a reported omission, or refused by name. No field and no
nested key is dropped silently.

## Why

llm-gateway 0.9.0 translates between the three wires with llm 0.6.0's ingress decoders and
encoders. Those decoders refuse fields Claude Code and Codex send, so a cross-wire route answers
`translation-unsupported` to both clients (`beyond10x/llm-gateway`
`crates/llm-gateway-cli/tests/translation.rs`, tests
`translation_known_limit_claude_codes_first_turn_is_outside_the_messages_subset` and
`translation_known_limit_codex_fields_are_outside_the_responses_subset`). Separately, the Chat and
Responses decoders pass over unknown keys below the top level, which AGENTS.md "Never" forbids
(the gateway closes those levels itself: `crates/llm-gateway-cli/src/translate.rs` `chat_closed`,
`responses_closed`).

## Evidence

- Claude Code first turn: the authored fixture
  `beyond10x/llm-gateway` `crates/llm-gateway/tests/fixtures/claude-code/1-tool-turn.request.txt`
  (adaptive `thinking`, `output_config.effort` `max`, a two-block `system` with `cache_control`,
  `cache_control` on a content block).
- Codex: requests captured on 2026-10-16 from `codex-cli 0.160.0` pointed at a loopback recorder
  that answered an error, so no paid call was made and no credential left the machine. The
  sanitized captures are the fixtures this epic's Responses story adds. Fields beside the 0.6.0
  subset: `instructions`, `parallel_tool_calls`, `reasoning.summary`, `prompt_cache_key`,
  `client_metadata`, `text.verbosity` (with a model Codex knows), `id` on every input item, a
  developer message of two text parts, and in the second turn a `function_call_output` whose
  output is a plain string.
- A Claude Code 2.1.301 first turn captured the same way carries, beside the fixture's fields,
  `metadata.user_id`, `thinking.display`, `context_management`, `safeguards` and a `system`
  message inside `messages`. Only `metadata` is given a carrying rule here; the rest is refused by
  name and listed as follow-up.

## Design

`docs/design.md` "Client request fields": the rule classes, the neutral additions
(`TurnRequest.controls`), the reasoning, tool-call and advisory tables, and the ingress rules.

## Stories

- `story:neutral-turn-controls` — llm-core: the controls, the effort table, `llm.turn/4`.
- `story:messages-ingress-client-fields` — Messages ingress and egress.
- `story:responses-ingress-client-fields` — Responses ingress and egress, the Codex fixtures.
- `story:chat-ingress-closed-levels` — Chat ingress closed at every level, the new options.
- `story:client-turns-cross-wire` — the two clients' first turns crossed to the other two wires.

## Follow-up, refused by name until then

From the Claude Code 2.1.301 capture: `thinking.display`, `context_management`, `safeguards`, a
`system` message inside `messages`, `output_config.format` (also in the fixture
`3-structured-output.request.txt`). From the Codex captures: a developer message after the first
input item (sent when a session is resumed), hosted and custom tools (`web_search`, `namespace`,
`custom`, `tool_search`), which a gateway user turns off in Codex.

## Out of scope

Structured output, server-side context editing, hosted and custom tools, `thinking.display`, an
instruction item in the middle of a conversation, and any change in llm-gateway (it moves to 0.7.0
on its own change).
