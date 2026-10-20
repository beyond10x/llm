---
format: aep.planning-md/3
id: story:tool-result-rejected-or-failed
kind: story
status: active
title: A rejected tool call has one documented encoding, and Chat ingress reads a failed result back as failed
summary: 'No wire carries a rejected state; document failed: true with the reason in output, and fix Chat ingress reading every tool message as succeeded'
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-chat/src/ingress.rs
- confidence: inferred
  path: crates/llm-chat/tests/ingress.rs
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
  path: spec/domains/chat.yaml
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/status.mdx
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T01:03:34Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T01:03:34Z", actor: "human:timo", revision: 6}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100): a caller that runs tools needs to tell the
model two different things, the tool ran and failed, or the call was rejected and never ran (refused
by a policy, an approval, or a bad argument). At tag 0.7.0 `Item::ToolResult`
(`crates/llm-core/src/item.rs:25-29`) carries `call_id`, `output` and `failed: bool`, so both are
`failed: true`; inference writes `{"failure": …}` or `{"rejected": …}` into `output`
(`crates/inference-llm/src/tools.rs:59-88` in that repository).

## Decision (design review of 2026-10-20)

No change to `Item`. `docs/plan/wave-2026-10-20-w09-design-review.md` § Layer 3 records the review;
the owner's delegate chose option B on 2026-10-20.

- No wire carries a rejected state: Messages has only `is_error`; Responses `function_call_output`
  and Chat tool messages carry text only (provider documentation, read 2026-10-20).
- A new key would be misread by a 0.7.0 reader: Responses reads an envelope with an extra key as a
  successful plain string (`crates/llm-responses/src/request.rs:793-801`), and Messages ingress
  refuses an unknown key (`crates/llm-messages/src/codec.rs:648`).
- The loops already keep the typed fact beside the item: inference `ToolOutcome`, Harness `refusal`
  (`harness-wire/src/item.rs:86-97`), Loom's copy of it.
- Changing `Item` would break 8 source and 22 test sites in llm and sites in Loom, inference and
  Harness for nothing a model sees.

The review found one defect: Chat ingress reads every tool message as succeeded
(`crates/llm-chat/src/ingress.rs:236-243`), so llm's own failure encoding,
`{"error": <output>, "ok": false}` (`crates/llm-chat/src/outgoing.rs:195-205`), reads back as a
success, which `docs/chat.md:47-51` does not say.

## Outcome

llm documents one encoding for a rejected call, `failed: true` with the reason in `output`, and Chat
ingress reads llm's own failure encoding back as a failure.

## Acceptance

- `docs/design.md` and `docs/{responses,messages,chat}.md` state that a call rejected before running
  is sent as `failed: true` with the reason as the caller's text in `output`, that no wire in scope
  carries a separate rejected state, and what each wire sends for a failed result.
- Chat ingress decodes a tool message whose content parses as a JSON object with exactly the keys
  `error` and `ok`, `ok` being `false`, as `failed: true` with `output` the `error` value. Any other
  content stays `failed: false` with the content as a string.
- A test encodes a failed result with the Chat encoder and decodes it with Chat ingress and gets
  `failed: true` and the original output back; a test pins that a success whose text is other JSON
  (for example `{"ok": false}` alone, or an object with a third key) stays a success.
- `spec/domains/chat.yaml` states the ingress rule; a conformance scenario pins it;
  `contracts/responses/scenarios/ingress-reads-an-envelope-with-an-extra-field-as-a-plain-string.yaml`
  and `contracts/chat/scenarios/outgoing-failed-tool-result-keeps-its-failure.yaml` are unchanged.

## Scope

Derived 2026-10-20 from the design review. **cited** = read in the tree; **inferred** = a reading.

- **Files:** `crates/llm-chat/src/ingress.rs:236-243` (the `tool` arm) — cited
- **Tests:** `crates/llm-chat/tests/ingress.rs` — inferred, home of the Chat ingress tests
- **Spec and contracts:** `spec/domains/chat.yaml`, a new scenario under `contracts/chat/scenarios/`, `contracts/ess-inputs.yaml`, regenerated `contracts/suite.json` — inferred
- **Documents:** `docs/design.md`, `docs/responses.md`, `docs/messages.md`, `docs/chat.md` — cited
- **Not in scope:** `crates/llm-core/src/item.rs`, every encoder, `llm.turn` — cited (the decision above)
- **Confidence:** high
