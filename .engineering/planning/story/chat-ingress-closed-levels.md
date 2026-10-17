---
format: aep.planning-md/3
id: story:chat-ingress-closed-levels
kind: story
status: implemented
title: Chat ingress refuses every unknown nested key by path and carries the new options
summary: closed levels, strict, parallel_tool_calls, prompt_cache_key, verbosity
relations:
- decomposes: epic:client-request-fields
- serves: vision:portable-model-inference
- depends_on: story:neutral-turn-controls
scope:
- confidence: cited
  path: checks/conformance/src/chat.rs
- confidence: cited
  path: contracts/chat/scenarios
- confidence: cited
  path: crates/llm-chat/src
- confidence: cited
  path: crates/llm-chat/tests
- confidence: cited
  path: docs/chat.md
- confidence: cited
  path: spec/domains/chat.yaml
revision: 14
transitions:
- {from: "draft", to: "proposed", at: "2026-10-17T04:25:35Z", actor: "human:timo", revision: 12, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "proposed", to: "active", at: "2026-10-17T04:25:36Z", actor: "human:timo", revision: 13, decided_on: {"recorded":{"review_outcome":2}}}
- {from: "active", to: "implemented", at: "2026-10-17T07:23:41Z", actor: "human:timo", revision: 14, decided_on: {"recorded":{"test_result":1,"review_outcome":2}}}
---
## Context

`llm_chat::decode_ingress_request` closes the top level and `stream_options`, but passes over any
key below them: a message's keys beyond the ones its role reads, content parts beyond `type` and
`text`, a tool call's keys beyond `id` and `function` (its `type` is not read), a function's
beyond `name` and `arguments`, a tool's beyond `type` and `function`, a tool function's beyond
`name`, `description` and `parameters` (so `strict: true` is dropped), and an object
`tool_choice`'s beyond `function.name` (`crates/llm-chat/src/ingress.rs` `decode_messages`,
`content_text`, `decode_tool_call`, `decode_tools`, `decode_tool_choice`). llm-gateway closes
those levels itself (`beyond10x/llm-gateway` `crates/llm-gateway-cli/src/translate.rs`
`chat_closed`). `parallel_tool_calls` is refused although Chat Completions has it.

## Design

`docs/design.md` "Client request fields", Chat column and the Chat ingress rule:

- Every object below the top is closed, refused by path through `llm_core::field_path`:
  `messages[]` by role (`system`/`developer`/`user`: `role`, `content`; `assistant`: `role`,
  `content`, `tool_calls`; `tool`: `role`, `content`, `tool_call_id`), `messages[].content[]`
  (`type`, `text`), `messages[].tool_calls[]` (`id`, `type`, `function`; `type` must be
  `function`), `messages[].tool_calls[].function` (`name`, `arguments`), `tools[]` (`type`,
  `function`), `tools[].function` (`name`, `description`, `parameters`, `strict`; `strict` true is
  refused, false or absent accepted), `tool_choice` (`type`, `function`; `type` must be
  `function`), `tool_choice.function` (`name`), `stream_options` (`include_usage`). The top-level
  refusal names the field too.
- Ingress and egress (`project_request`): `parallel_tool_calls` → controls, `prompt_cache_key` and
  `verbosity` → hints, each re-emitted under the same name; effort through `llm_core::wire_effort`
  (`max` → `xhigh`, disabled thinking → `none`); an enabled thinking budget, and disabled thinking
  beside an effort, refused. `parallel_tool_calls` leaves `NAMED_UNSUPPORTED_REQUEST_FIELDS`.
- `omitted_advisories(&TurnRequest)` exported (`prompt-cache`, `reasoning-summary` and the
  unforwarded names); `ChatClient` emits `llm_core::advisory_warning` for them after a
  successful turn.

ESS first: `spec/domains/chat.yaml` gains the observed `wire_request`, `turn_json` and
`omitted_advisories`.

## Shared files

Besides `crates/llm-chat`: `spec/domains/chat.yaml`, `checks/conformance/src/chat.rs`,
`contracts/chat/scenarios/`, `docs/chat.md`. The coordinator owns
`contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/schema/` and
`contracts/baseline.json`: the branch lists its scenarios there and regenerates the projections
only in an uncommitted working copy to run its own check, and the coordinator lists them and
regenerates once on the merged wave.

## Acceptance

- An unknown key at each closed level is refused naming its path, one test per level; `strict`
  true is refused, and a non-`function` tool-call or tool-choice `type` is refused.
- Round trips: `parallel_tool_calls`, `prompt_cache_key` and `verbosity` decode and
  `project_request` re-emits them; the projected body decodes back to an equal turn.
- Effort `max` projects as `xhigh`; disabled thinking as `none`; an enabled budget is refused.
- `omitted_advisories` returns, in the design's order, exactly the present hints among `prompt-cache`,
  `reasoning-summary` and the unforwarded names; `ChatClient` emits one `advisory-omitted` warning
  naming them after a successful turn, and none when nothing is omitted or the turn failed.
- Conformance scenarios in `contracts/chat/scenarios/` for each, passing on the merged wave with
  none skipped.

## Verification

`cargo test -p b10x-llm-chat`; `cargo clippy -p b10x-llm-chat --all-targets -- -D warnings`;
`task conformance` on the merged wave.
