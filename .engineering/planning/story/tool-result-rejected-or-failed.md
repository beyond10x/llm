---
format: aep.planning-md/3
id: story:tool-result-rejected-or-failed
kind: story
status: active
title: A tool result says whether the call failed while running or was rejected before running
summary: Item::ToolResult has output and failed only; a rejected call and a failed one look the same on every wire
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: checks/conformance/src/chat.rs
- confidence: cited
  path: checks/conformance/src/messages.rs
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/chat/scenarios/ingress-a-client-request-decodes-onto-the-neutral-subset.yaml
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/messages/scenarios/ingress-preserves-the-declared-subset.yaml
- confidence: inferred
  path: contracts/messages/scenarios/request-preserves-roles-tools-and-thinking.yaml
- confidence: inferred
  path: contracts/responses/scenarios/a-failed-tool-result-keeps-its-failure.yaml
- confidence: inferred
  path: contracts/schema
- confidence: cited
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-chat/src/ingress.rs
- confidence: cited
  path: crates/llm-chat/src/outgoing.rs
- confidence: inferred
  path: crates/llm-chat/tests/ingress.rs
- confidence: inferred
  path: crates/llm-chat/tests/outgoing.rs
- confidence: cited
  path: crates/llm-core/src/item.rs
- confidence: inferred
  path: crates/llm-core/src/turn.rs
- confidence: inferred
  path: crates/llm-core/tests/controls.rs
- confidence: inferred
  path: crates/llm-core/tests/embedding.rs
- confidence: cited
  path: crates/llm-messages/src/codec.rs
- confidence: inferred
  path: crates/llm-messages/tests/ingress.rs
- confidence: inferred
  path: crates/llm-messages/tests/projection.rs
- confidence: inferred
  path: crates/llm-messages/tests/subscription.rs
- confidence: cited
  path: crates/llm-responses/src/request.rs
- confidence: inferred
  path: crates/llm-responses/tests/adversary_w08_fields.rs
- confidence: inferred
  path: crates/llm-responses/tests/client_fields.rs
- confidence: inferred
  path: crates/llm-responses/tests/ingress.rs
- confidence: inferred
  path: crates/llm-responses/tests/projection.rs
- confidence: inferred
  path: crates/llm-responses/tests/wire_parity.rs
- confidence: inferred
  path: crates/llm-responses/tests/wire_support/mod.rs
- confidence: cited
  path: docs/chat.md
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: docs/responses.md
- confidence: inferred
  path: docs/verification/messages-falsification.json
- confidence: inferred
  path: spec/domains/chat.yaml
- confidence: inferred
  path: spec/domains/messages.yaml
- confidence: cited
  path: spec/domains/responses.yaml
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/concepts/protocols.md
- confidence: inferred
  path: website/docs/status.mdx
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T01:03:34Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T01:03:34Z", actor: "human:timo", revision: 6}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100). A caller that runs tools needs to tell the
model two different things: the tool ran and failed, or the call was rejected and never ran (refused
by a policy, an approval, or a bad argument).

At tag 0.7.0 `Item::ToolResult` (`crates/llm-core/src/item.rs:25-29`) carries `call_id`, `output`
and `failed: bool`, so both cases are `failed: true`. inference works around it by writing
`{"failure": …}` or `{"rejected": …}` into `output`.

Each wire encodes `failed` its own way: Messages sets `is_error` (`crates/llm-messages/src/codec.rs:51`),
Responses wraps the output as `{"ok": !failed, "output": …}` (`crates/llm-responses/src/request.rs:641-642`),
Chat writes text (`crates/llm-chat/src/outgoing.rs:195`). None of the three wires has a field for
"rejected", and the ingress decoders (`request.rs:747`, `codec.rs:649`) read back only failed or not.

## Outcome

A caller states whether a tool call succeeded, failed while running, or was rejected before running;
each wire carries the distinction in a form the model reads; an ingress decoder that cannot tell
failure from rejection reports it as undetermined, never as rejected.

## Acceptance

- The neutral tool result carries the three cases; serialized items written before the change still
  decode (`failed: true` reads as failed, `false` as succeeded).
- Per protocol, a test encodes a rejected result and asserts the wire body; a test decodes each wire's
  error form and asserts it never becomes "rejected".
- The ESS specification, `contracts/`, `docs/design.md` and the three protocol pages describe the
  encoding; `CHANGELOG.md` names the change as breaking if the Rust type changes shape.

## Scope

Derived 2026-10-20 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-core` (`Item::ToolResult`) and the request codecs of the three protocol crates — cited
- **Files:** `crates/llm-core/src/item.rs:25-29` (the variant; `deny_unknown_fields` at `:17`) — cited
- **Files:** `crates/llm-messages/src/codec.rs:46-52` (encode, `is_error`), `:647-664` (decode builds `failed`) — cited
- **Files:** `crates/llm-responses/src/request.rs:604-612`, `:641-645` (`tool_result_envelope`), `:741-752`, `:793-801` (`read_tool_result`) — cited
- **Files:** `crates/llm-chat/src/outgoing.rs:152-162`, `:195-205` (`result_text`); `crates/llm-chat/src/ingress.rs:236-243` (decodes every `tool` message as `failed: false`) — cited
- **Files:** `checks/conformance/src/chat.rs:415-419` and `checks/conformance/src/messages.rs:352-356` read `failed` into the item labels the scenarios assert — cited
- **Match sites that compile unchanged when a field changes** (they use `..`): `crates/llm-core/src/turn.rs:137`, `:375`; `crates/llm-messages/src/codec.rs:108`; `crates/llm-messages/src/ingress.rs:573`; `crates/llm-responses/src/ingress.rs:365`, `:560`, `:725`; `crates/llm-chat/src/ingress.rs:742`; `checks/conformance/src/responses.rs:475` — cited
- **Symbols:** `Item::ToolResult`, `tool_result_envelope`, `read_tool_result`, `result_text`, the Messages `"tool_result" if user` arm — cited
- **Also likely:** `crates/llm-core/src/turn.rs:425-498` (`TurnFormat`); a new state probably needs a new `llm.turn` version, as `UnattributedOpaque` took `/2` to `/3` (`crates/llm-core/tests/embedding.rs:234`) — inferred
- **Also likely (tests):** 22 `Item::ToolResult { …, failed }` literals in 13 test files under `crates/llm-{core,chat,messages,responses}/tests/`, if the variant's fields change — inferred
- **Also likely (spec):** `spec/domains/responses.yaml:95-105` describes the envelope; `spec/domains/messages.yaml` and `spec/domains/chat.yaml` say nothing about failed results yet — cited
- **Also likely (contracts):** new scenarios in `contracts/ess-inputs.yaml`, `contracts/baseline.json`; existing scenarios that change if `is_error: true` or `{"ok":false}` decodes as undetermined: `contracts/messages/scenarios/{ingress-preserves-the-declared-subset,request-preserves-roles-tools-and-thinking}.yaml`, `contracts/chat/scenarios/ingress-a-client-request-decodes-onto-the-neutral-subset.yaml`, `contracts/responses/scenarios/a-failed-tool-result-keeps-its-failure.yaml` — inferred
- **Generated:** `contracts/suite.json` — cited; `contracts/schema/` only if spec fields change — inferred
- **Also likely (coupled check):** `docs/verification/messages-falsification.json`; `crates/llm-messages/tests/falsification.rs:68-88` requires an entry for every refusal literal in `crates/llm-messages/src/*.rs` — inferred
- **Documents:** `docs/design.md` (`:89-90`, `:150-151`), `docs/responses.md:144-156`, `docs/messages.md:20`, `docs/chat.md:26,47-51`, `CHANGELOG.md` — cited; `website/docs/concepts/protocols.md:52-53,112-113`, `docs/implementation-status.md` and its generated status files — inferred
- **Pinned, must not change:** `crates/llm-core/tests/fixtures/llm-0.6.0-turn.json`, `contracts/anthropic/subscription-request.json`, `contracts/chat/scenarios/outgoing-failed-tool-result-keeps-its-failure.yaml`, `contracts/responses/scenarios/ingress-reads-an-envelope-with-an-extra-field-as-a-plain-string.yaml` — cited
- **Not in scope:** `crates/llm-tool-call` (only `Item::ToolCall`, `src/lib.rs:420`), `llm-blocking`, `llm-cli`, `llm-docs` examples, `llm-routing` — cited
- **Confidence:** medium; tests, scenarios and generated files depend on two open choices (changed field or new variant; new `llm.turn` version or not)
- **Would collide with:** any unit changing `llm_core::Item` or the `llm.turn` format; Messages `codec.rs`, Responses `request.rs`, Chat `outgoing.rs`/`ingress.rs`; the conformance runner's item labels; every scenario-adding unit through `contracts/`; every unit through `CHANGELOG.md` and `docs/implementation-status.md` — inferred
- **Safety fact:** the two existing states keep their bytes (`a_turn_without_controls_is_written_as_llm_0_6_0_wrote_it`, `crates/llm-core/tests/controls.rs:91`). Responses `read_tool_result` (`request.rs:793-801`) reads any envelope that is not exactly `{ok, output}` as a successful plain string, and Chat ingress (`ingress.rs:241-242`) reads every tool message as succeeded, so a rejection written as an extra envelope key or as Chat text reads back as success on a 0.7.0 gateway — cited, unproven

## Open for the implementor

- A changed field or a new variant; a new variant leaves the 13 test files alone and moves the forced
  edits to the exhaustive matches listed above.
- Whether a rejected result needs `llm.turn/5`.
- Whether Chat ingress starts parsing the `{"error":…,"ok":false}` text the encoder writes, so
  failures read back as undetermined rather than succeeded.
