---
format: aep.planning-md/3
id: story:responses-ingress-client-fields
kind: story
status: active
title: Responses ingress and egress hold Codex's requests under written rules, closed at every level
summary: instructions, parallel_tool_calls, prompt_cache_key, text.verbosity, reasoning.summary, item ids, client_metadata, plain-string results
relations:
- decomposes: epic:client-request-fields
- serves: vision:portable-model-inference
- depends_on: story:neutral-turn-controls
scope:
- confidence: cited
  path: checks/conformance/src/responses.rs
- confidence: cited
  path: contracts/responses/scenarios
- confidence: cited
  path: crates/llm-responses/src
- confidence: cited
  path: crates/llm-responses/tests
- confidence: cited
  path: docs/responses.md
- confidence: cited
  path: spec/domains/responses.yaml
revision: 13
transitions:
- {from: "draft", to: "proposed", at: "2026-10-17T04:25:34Z", actor: "human:timo", revision: 12, decided_on: {"recorded":{"review_outcome":4}}}
- {from: "proposed", to: "active", at: "2026-10-17T04:25:35Z", actor: "human:timo", revision: 13, decided_on: {"recorded":{"review_outcome":4}}}
---
## Context

`llm_responses::ingest_request` refuses the top-level fields Codex sends beside the subset
(`ACCEPTED_BODY_FIELDS`, `crates/llm-responses/src/request.rs`) and `reasoning.summary`
(`sampling`), and passes over unknown keys inside `input[]` items, their content parts and an
object `tool_choice` (`input_to_item`, `content_text`, `wire_to_tool_choice`). Codex 0.160.0,
captured on 2026-10-16 against a loopback recorder, sends `instructions`, `parallel_tool_calls`,
`reasoning.summary`, `prompt_cache_key`, `client_metadata`, `text.verbosity` (for a model it
knows), an `id` on every input item and a developer message of two text parts; its second turn
carries a `function_call_output` whose output is a plain string.

## Design

`docs/design.md` "Client request fields", Responses column and the Responses ingress rule:

- Ingress: `instructions` (a string) then the `input_text` parts of every developer message that
  opens `input`, joined by `\n`, → instructions. `id` (a string) on a `message`, `function_call` or
  `function_call_output` item → `hints.unforwarded` `input-item-ids`. `parallel_tool_calls`
  (boolean) → controls. `prompt_cache_key`, `text` (`verbosity` only; `format` and any other key
  refused by path), `reasoning.summary` → hints. `client_metadata` (an object) → unforwarded
  `client-metadata`; its value is never kept. A `function_call_output` string that is exactly the
  pinned envelope is read as one; any other string is a successful result carrying that string.
- Closed at every level, refused by path through `llm_core::field_path`: each `input[]` item type,
  content parts (`annotations` accepted only as an empty array on `output_text`), an object
  `tool_choice`, `tools[]`, `reasoning`, `text`, and the top level. A tool of another type than
  `function` is refused naming that type when it is one of `web_search`, `custom`, `namespace`,
  `tool_search`, `local_shell`, `file_search`, `computer_use_preview`, `mcp`,
  `image_generation`, `code_interpreter`.
- Egress (`project_request`): `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity`,
  `reasoning.summary`; effort through `llm_core::wire_effort`; an enabled thinking budget, and
  disabled thinking beside an effort, refused. `encode_request` refuses a conversation whose
  identifier differs from the turn's `prompt_cache_key`.
- `omitted_advisories(&TurnRequest)` exported (`prompt-cache` and the unforwarded names);
  `ResponsesClient` emits `llm_core::advisory_warning` for them after a successful turn.
- Fixtures: `crates/llm-responses/tests/fixtures/codex/first-turn.json` and `second-turn.json`, the
  captured first and second Codex turns, sanitized (identifiers replaced, text
  values shortened, no home path), with a provenance note naming the capture.

ESS first: `spec/domains/responses.yaml` (the rows on `prompt_cache_key` and on tool results
change; the observed `omitted_advisories`).

## Shared files

Besides `crates/llm-responses`: `spec/domains/responses.yaml`, `checks/conformance/src/responses.rs`,
`contracts/responses/scenarios/`, `docs/responses.md`. The coordinator owns
`contracts/ess-inputs.yaml`, `contracts/suite.json`, `contracts/schema/` and
`contracts/baseline.json`: the branch lists its scenarios there and regenerates the projections
only in an uncommitted working copy to run its own check, and the coordinator lists them and
regenerates once on the merged wave.

## Acceptance

- The captured first Codex turn ingests: instructions are `instructions` and the developer
  message's two parts joined by `\n`; `parallel_tool_calls` true; `reasoning.summary` `auto`;
  `prompt_cache_key`; unforwarded `input-item-ids` and `client-metadata`; four function tools.
- The captured second turn ingests: the reasoning item carried unattributed, the call, and a
  successful result carrying the plain string.
- Round trips, each ingested, re-projected with `project_request` and ingested back to an equal
  turn: `instructions` alone, `parallel_tool_calls` true and false, `prompt_cache_key`,
  `text.verbosity` (an authored copy of the first turn with `{"verbosity":"low"}`, the shape a
  model Codex knows was captured with), `reasoning.summary`, effort `max` sent as `xhigh`. The
  one-way rules are asserted as one-way: `instructions` with developer parts comes back as one
  developer message, item ids and `client_metadata` are not re-emitted and are reported omitted,
  a plain-string result comes back in the pinned envelope.
- `omitted_advisories` returns, in the design's order, exactly the present hints among
  `prompt-cache` and the unforwarded names (for the first Codex turn: `input-item-ids`,
  `client-metadata`); `ResponsesClient` emits one `advisory-omitted` warning naming them after a
  successful turn, and none when nothing is omitted or the turn failed.
- Egress refusals: an enabled thinking budget, disabled thinking beside an effort, and a
  conversation whose identifier differs from the turn's `prompt_cache_key`.
- Refused by path: a key added to each closed level, `text.format`, `reasoning.generate_summary`,
  a `web_search` tool by its type, a developer message after the first item.
- Conformance scenarios in `contracts/responses/scenarios/` for each, passing on the merged wave
  with none skipped.

## Verification

`cargo test -p b10x-llm-responses`; `cargo clippy -p b10x-llm-responses --all-targets -- -D
warnings`; `task conformance` on the merged wave.
