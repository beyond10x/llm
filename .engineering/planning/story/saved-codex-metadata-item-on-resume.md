---
format: aep.planning-md/3
id: story:saved-codex-metadata-item-on-resume
kind: story
status: implemented
title: A response.metadata item saved before 0.17.0 still reaches the endpoint on resume
summary: Opaque items saved under 0.16.0 carry the stream event; egress leaves it out (adversary finding D202, decided D208)
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T13:15:22Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-29T13:15:22Z", actor: "human:timo", revision: 5}
- {from: "active", to: "implemented", at: "2026-10-29T13:30:07Z", actor: "human:timo", revision: 7, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

Found by the adversary pass on `story:codex-stream-metadata-is-not-replayed` (wave 2026-10-29-w21,
D202). Since that story no outcome carries a `response.metadata` event. A conversation a caller
saved under llm 0.16.0 or earlier (a Harness session continued with `--resume`) still holds the
event as an `Item::Opaque` of the Codex binding, and `project_request` sends every opaque item of
the binding verbatim, so the resumed session's next request carries the entry the endpoint refuses
with HTTP 400 "Invalid value: 'response.metadata'".

Pinned until this story by
`crates/llm-responses/tests/adversary_w21_metadata.rs::a_metadata_item_kept_from_an_earlier_turn_is_not_replayed_as_an_input_entry`.

## Options

- a. Egress refuses, before any I/O, an opaque item whose payload is a stream event rather than an
  output item, naming it. Clearer than the endpoint's 400; the session still cannot continue.
- b. Harness drops such items from a saved session on resume (its own store; Harness invariant 5
  governs opaque items it replays).
- c. Nothing: sessions saved before the fix are rare and short-lived.
- d. Egress leaves out a saved `response.metadata` item of the selected binding and sends every
  other item as before.

## Decision

Option d, by orchestrator item D208 (`approval-record:wave-2026-10-29-w22-proposal`): "do not send
a saved metadata item; do not drop any other item kind". Opaque state of another binding is still
refused before anything is left out.

## Acceptance

- A conversation saved by llm 0.16.0 from the recorded live Codex turn
  (`crates/llm-responses/tests/fixtures/live/codex-text-turn-saved-0.16.0.json`), continued with a
  second user message, projects to a request whose `input` carries no `response.metadata` entry and
  carries the reasoning item, the answer and both user messages unchanged and in order.
- The pinned adversary case asserts that no `response.metadata` entry is sent.
- Scenario `contracts/responses/scenarios/egress-leaves-out-a-saved-metadata-item.yaml` holds the
  projection: accepted, the metadata entry absent, every other entry in order.
- A saved metadata item of another binding is still refused; every other opaque item, including
  one whose `type` is another stream event, is still sent verbatim.
