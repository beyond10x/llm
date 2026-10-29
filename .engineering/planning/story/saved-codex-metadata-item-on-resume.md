---
format: aep.planning-md/3
id: story:saved-codex-metadata-item-on-resume
kind: story
status: draft
title: A response.metadata item saved before 0.17.0 still reaches the endpoint on resume
summary: Opaque items saved under 0.16.0 carry the stream event; project_request still sends them verbatim (adversary finding, D202)
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 1
---
## Context

Found by the adversary pass on `story:codex-stream-metadata-is-not-replayed` (wave 2026-10-29-w21,
D202). Since that story no outcome carries a `response.metadata` event. A conversation a caller
saved under llm 0.16.0 or earlier (a Harness session continued with `--resume`) still holds the
event as an `Item::Opaque` of the Codex binding, and `project_request` sends every opaque item of
the binding verbatim, so the resumed session's next request carries the entry the endpoint refuses
with HTTP 400 "Invalid value: 'response.metadata'".

Pinned today by `crates/llm-responses/tests/adversary_w21_metadata.rs::a_metadata_item_kept_from_an_earlier_turn_is_not_replayed_as_an_input_entry`.

## Options

- a. Egress refuses, before any I/O, an opaque item whose payload is a stream event rather than an
  output item, naming it. Clearer than the endpoint's 400; the session still cannot continue.
- b. Harness drops such items from a saved session on resume (its own store; Harness invariant 5
  governs opaque items it replays).
- c. Nothing: sessions saved before the fix are rare and short-lived.

Not decided in D202; the fix there needs none of these.

## Acceptance

The chosen option, with the pinned test rewritten to it.
