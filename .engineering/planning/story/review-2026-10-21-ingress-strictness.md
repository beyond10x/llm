---
format: aep.planning-md/3
id: story:review-2026-10-21-ingress-strictness
kind: story
status: active
title: Responses and chat ingress refuse an omitted tool schema and an omitted text part text
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-21T11:12:54Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-21T11:12:54Z", actor: "human:timo", revision: 3}
---
## Context

The 2026-10-21 layer review of llm (`findings-llm.md`, findings 2, 5 and 6, checked against
`0f960cdc`) found two ingress decoders that accept an omission instead of refusing it, and one
ignored test that asserts the opposite of the documented behaviour:

- `crates/llm-responses/src/request.rs:560`: a Responses `function` tool without `parameters`
  becomes a tool with a null input schema. The chat decoder refuses the same omission
  (`crates/llm-chat/src/ingress.rs:375`), and `docs/responses.md:97` lists `parameters` as part of
  the tool.
- `crates/llm-chat/src/ingress.rs:450`: a chat `text` content part with no `text`, or a null one,
  is read as empty text.
- `crates/llm-responses/tests/adversary_codex_stream.rs:527`: an ignored test asserts that an
  empty `auto` turn is refused, while its doc comment and `docs/responses.md` accept it as an
  `EndTurn` turn with no items.

The review's findings 1, 3, 4 and 7 are declined: the renewal compare-then-rename window is the
documented design (`docs/harness-parity.md` C14), the directory fsync after a landed rename stays
`Ok`, the spawned-task refusal was declined in wave 2026-10-04-w17, and finding 3 was false.

The only consumer of either ingress decoder is llm-gateway (`translate.rs:364`, `:378`); no
committed body in llm-gateway, inference, loom, takt, epistemic-knowledge-runtime, metaharness,
uilab or tools sends either omitted shape.

## Outcome

Both decoders refuse the omission, as invalid requests, and the ignored test pins the documented
empty turn.

## Acceptance

- `ingest_request` refuses a `function` tool without a `parameters` key with an invalid-request
  error naming the omission; a present `null` is refused, as before, by the turn's schema check.
- `decode_ingress_request` refuses a `text` content part whose `text` is absent or null with an
  invalid-request error; a string, empty included, is read as before.
- The ignored test in `adversary_codex_stream.rs` runs and asserts an `EndTurn` turn with no items.
- `docs/responses.md` and `docs/chat.md` state both refusals; `CHANGELOG.md` `[Unreleased]` names
  them under `Changed`.
- `task check` exits 0.
