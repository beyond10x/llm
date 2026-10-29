---
format: aep.planning-md/3
id: story:codex-stream-metadata-is-not-replayed
kind: story
status: implemented
title: A Codex stream's response.metadata event is not replayed as an input item
summary: response.metadata joins the pinned stream subset as a non-conversation event, so no next request carries it (D202)
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T11:27:33Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-29T11:27:33Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-29T12:03:50Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":3,"verification":1}}}
---
## Context

Orchestrator item D202, from Harness wave D200 (beyond10x/harness pull request 59, merge
`3b88c4d`; Harness draft `story:responses-replays-no-stream-metadata`). A live `-p codex` run on
`gpt-5.6-sol` through llm 0.16.0 ended every tool-using run on its second turn with HTTP 400
`invalid_value`, param `input[6]`: "Invalid value: 'response.metadata'".

The Codex backend streams a `response.metadata` event. It is outside `ACCEPTED_STREAM_EVENTS`, so
`crates/llm-responses/src/stream.rs` warns `unknown-stream-event`, turns the whole event into an
`Item::Opaque` and puts it in the turn's items. The caller replays opaque items verbatim (Harness
invariant 5), `encode_request` writes the payload as an input entry, and the endpoint refuses an
input of type `response.metadata`.

## Decision

`response.metadata` joins the pinned subset as a non-conversation event: interpreted, not
preserved as an item, so no outcome carries it and no next request replays it. It describes the
response; it is not output. The general rule for an event outside the pinned subset (preserved as
an opaque item and warned about, `docs/responses.md` "The pinned stream subset", scenario
`an-event-outside-the-pinned-subset-is-preserved`) is a documented contract and stays unchanged.
Retry eligibility is unchanged too: the event is not a lifecycle event, as it was not before.

## Acceptance

- `crates/llm-responses/tests/codex_stream.rs::a_metadata_event_is_not_conversation_state_and_is_not_replayed`:
  a Codex-shaped stream (function call, `response.metadata`, `response.completed` with `output: []`)
  decodes to the call alone, with no `unknown-stream-event` warning, and the next request
  `encode_request` builds from it plus the tool result carries no entry of type
  `response.metadata`.
- Scenario `contracts/responses/scenarios/a-metadata-event-is-not-conversation-state.yaml`.
- `crates/llm-responses/tests/projection.rs::an_event_outside_the_pinned_subset_is_preserved_and_reported`
  still passes unchanged.
