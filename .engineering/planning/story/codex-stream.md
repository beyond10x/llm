---
format: aep.planning-md/3
id: story:codex-stream
kind: story
status: proposed
title: The Responses client completes a turn against the Codex backend
relations:
- decomposes: epic:access
- depends_on: story:responses-client
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: crates/llm-http/src/transport.rs
- confidence: cited
  path: crates/llm-responses/src/stream.rs
- confidence: cited
  path: crates/llm-responses/tests/codex_stream.rs
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-04T17:22:35Z", actor: "human:timo", revision: 5}
---

## Outcome

`ResponsesClient` completes a turn against the Codex backend (`https://chatgpt.com/backend-api/codex`).
A live probe on 2026-10-04 (four runs, one request each, through llm 0.1.5) showed the backend
accepts the request llm sends: no extra header, no other body field, model `gpt-5.6-sol` accepted.
llm refuses the answer in two places:

1. **No content type.** The backend answers `200` with a valid server-sent event stream and no
   `content-type` header. `llm-http` refuses any 2xx that is not `text/event-stream`
   ("HTTP success response is not text/event-stream", `crates/llm-http/src/transport.rs`). A 2xx
   with no `content-type` to a request that asked for `text/event-stream` is read as an event stream;
   a 2xx that names another content type is still refused.
2. **Empty terminal output.** The stream ends in `response.completed` whose `response.output` is
   `[]`; the function call arrived only in `response.output_item.done`. The decoder takes the
   terminal `output` whenever it is an array (`crates/llm-responses/src/stream.rs`, `terminate`), so
   the streamed call is dropped and the turn is refused ("a finished function call contradicts its
   announcement"). An empty terminal `output` after streamed items falls back to the streamed
   items; a non-empty terminal `output` keeps today's rule.

## Acceptance

`a_codex_style_stream_completes_its_turn`, against a local HTTP fixture that answers like the probe
recorded (no `content-type`, a `function_call` streamed through `output_item.done`, then
`response.completed` with `output: []`): the turn returns the call with its arguments, stop reason
`ToolCalls`, final usage. Beside it: a 2xx with `content-type: application/json` is still refused;
an empty terminal `output` with no streamed items is still refused as a turn without output.

## ESS first

None: the transport and stream entities keep their states and outcomes. If a spec scenario pins
"not text/event-stream" for a missing header, it changes with the code and says so.

## Evidence

The probe's source and log are outside the repository (`~/.cache/intake-codex-probe`); the
fixture in the acceptance reproduces the recorded headers and event order. First user:
beyond10x/intake `story:model-access` live check.
