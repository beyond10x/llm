---
format: aep.planning-md/3
id: story:capabilities-declare-streaming
kind: story
status: active
title: A model's declaration says whether its output streams and whether input can reach a running turn
summary: Capabilities cannot declare output streaming or input during output; a graph node over Model cannot schedule from the declaration
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: inferred
  path: checks/conformance/src/routes.rs
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/providers/scenarios
- confidence: inferred
  path: contracts/routes/scenarios
- confidence: inferred
  path: contracts/routing/scenarios
- confidence: cited
  path: contracts/schema
- confidence: cited
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-blocking/tests/blocking.rs
- confidence: cited
  path: crates/llm-chat/tests/common/mod.rs
- confidence: cited
  path: crates/llm-core/src/vocabulary.rs
- confidence: inferred
  path: crates/llm-core/tests
- confidence: cited
  path: crates/llm-messages/tests/support/mod.rs
- confidence: cited
  path: crates/llm-models/src/routes.rs
- confidence: inferred
  path: crates/llm-models/tests/routes.rs
- confidence: inferred
  path: crates/llm-providers/src/binding.rs
- confidence: inferred
  path: crates/llm-providers/tests/bindings.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary2_client.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_client.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_codex_stream.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w27_retry.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w31_live.rs
- confidence: cited
  path: crates/llm-responses/tests/client.rs
- confidence: cited
  path: crates/llm-responses/tests/codex_stream.rs
- confidence: cited
  path: crates/llm-responses/tests/live_stream.rs
- confidence: cited
  path: crates/llm-responses/tests/retry_class.rs
- confidence: cited
  path: crates/llm-responses/tests/wire_support/mod.rs
- confidence: inferred
  path: crates/llm-routing/tests/catalog.rs
- confidence: cited
  path: crates/llm-tool-call/src/lib.rs
- confidence: cited
  path: crates/llm-tool-call/tests/adversary_model.rs
- confidence: cited
  path: crates/llm-tool-call/tests/forced_tool_call.rs
- confidence: cited
  path: docs/design.md
- confidence: cited
  path: docs/implementation-status.md
- confidence: cited
  path: spec/domains/catalog.yaml
- confidence: inferred
  path: spec/domains/routes.yaml
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/guides/configure-model-routes.md
- confidence: inferred
  path: website/docs/guides/explain-a-route.md
- confidence: inferred
  path: website/docs/reference/formats.md
- confidence: inferred
  path: website/docs/status.mdx
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T01:03:33Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T01:03:33Z", actor: "human:timo", revision: 6}
---
## Context

Reported by the `inference` repository while it builds its LLM graph node as a thin wrapper over
`llm_core::Model` (Atlas ADR 0100). A graph decides how to schedule a node from what the node
declares, and the declaration it reads is `Model::capabilities()`.

At tag 0.7.0, `Capabilities` (`crates/llm-core/src/vocabulary.rs:46-58`) declares tools, tool choice,
temperature, top-p, reasoning efforts, context window and output limit. It cannot declare:

- whether the model's text reaches the sink incrementally, as it is generated, or only at the end;
- whether input can reach a turn that is still producing output.

All three clients in this workspace request a streamed answer today (Responses
`crates/llm-responses/src/request.rs:153`, Messages `crates/llm-messages/src/codec.rs:148`, Chat
`crates/llm-chat/src/client.rs:61`), so for them the first answer is always "yes" and the second
always "no". A `Model` implemented elsewhere can differ, and a caller cannot tell. inference works
around this with claims of its own beside the model.

`Capabilities` is `deny_unknown_fields` and is read from catalog, binding and routes documents
(`llm-providers`, `llm-models`), so a new field is a change to those documents too.

## Outcome

A caller reads from a model's `Capabilities` whether its output streams and whether it accepts input
during output, and every client in this workspace declares both truthfully.

## Acceptance

- `Capabilities` carries both declarations; `Capabilities::text` and every client in the workspace
  set them to what the client does (streaming yes, input during output no).
- A catalog, binding or routes document written before this change still loads, with the values the
  clients declare; a document that claims input during output for a route whose protocol cannot
  carry it is refused.
- The ESS specification, `contracts/` and `docs/design.md` describe both declarations.

## Scope

Derived 2026-10-20 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-core` (`Capabilities`) — cited
- **Files:** `crates/llm-core/src/vocabulary.rs:46-89` (the struct, `Capabilities::text`, `validate`; serde defaults so a document written before the change loads as streaming yes, input during output no) — cited
- **Files:** `crates/llm-providers/src/binding.rs:43-68` `BindingDocument::bind`, the only place that sees `protocol` and `capabilities` together, as the site of the protocol refusal — inferred
- **Files:** `crates/llm-models/src/routes.rs:75-85` `default_capabilities` lists every field and stops compiling once fields are added — cited
- **Files:** `crates/llm-models/src/routes.rs:715-733` a refusal that names the route, plus the module doc at `:1-35` — inferred
- **Files:** `crates/llm-tool-call/src/lib.rs:200-208` lists every field (compile-forced) — cited
- **Files:** `spec/domains/catalog.yaml:18-27` `llm.catalog.Capabilities` — cited
- **Generated:** `contracts/schema` (all 204 files rewritten for the whole-spec `source_digest`; content changes in `types/llm.catalog.Capabilities`, `types/llm.catalog.TargetExplanation`, `types/llm.catalog.RouteExplanation`, `entities/llm.catalog.ServingModel`) and `contracts/suite.json` — cited
- **Generated:** `website/data/status.json`, `website/docs/status.mdx`, if a `## Status at` row changes — inferred
- **Documents carrying `Capabilities`:** `llm.binding/1` (`crates/llm-providers/src/binding.rs:15-22`, `declaration.rs:186-193`), `llm.catalog/1` (`crates/llm-routing/src/catalog.rs:45`), `llm.routes/1` (`crates/llm-models/src/routes.rs:121-126`), route explanation output (`crates/llm-routing/src/selection.rs:46`), Chat conformance `capabilities_json` (`spec/domains/chat.yaml:136`); catalog validation (`catalog.rs:111-125`) and routes expansion (`routes.rs:645-654`) both go through `bind` — cited
- **Tests that list every field (compile-forced):** `crates/llm-blocking/tests/blocking.rs:86`, `crates/llm-chat/tests/common/mod.rs:14`, `crates/llm-messages/tests/support/mod.rs:14`, ten files under `crates/llm-responses/tests/` (`adversary2_client.rs`, `adversary_client.rs`, `adversary_codex_stream.rs`, `adversary_w27_retry.rs`, `adversary_w31_live.rs`, `client.rs`, `codex_stream.rs`, `live_stream.rs`, `retry_class.rs`, `wire_support/mod.rs`), `crates/llm-tool-call/tests/{adversary_model.rs, forced_tool_call.rs}` — cited
- **Tests pinning documents, where the acceptance cases go:** `crates/llm-providers/tests/bindings.rs`, `crates/llm-routing/tests/catalog.rs`, `crates/llm-models/tests/routes.rs`, a new defaults case in `crates/llm-core/tests` — inferred
- **Old-document evidence that needs no edit:** pre-change `llm.binding/1` JSON at `crates/llm-providers/tests/line_terminator.rs:12`, `adversary_line_terminator.rs:12`, `refusal_names_reference.rs:9`, and `examples/catalog.toml` — cited
- **Conformance:** new refusal scenarios in `contracts/{providers,routing,routes}/scenarios`, listed in `contracts/ess-inputs.yaml`; `contracts/baseline.json` floor — inferred
- **Conditional on serialization:** if the new fields always serialize, `config_digest` `1365b05f…` (2 routing scenarios), `binding_revision` `1a6ea40f…` (5 opaque-state scenarios) and `website/docs/guides/explain-a-route.md:26,40,44,69` move — inferred
- **Conditional on the routes projection:** `checks/conformance/src/routes.rs:73-84`, `spec/domains/routes.yaml:63-67`, two `contracts/routes/scenarios/a-*-capabilities.yaml` — inferred
- **Documents:** `docs/design.md`, `CHANGELOG.md`, `docs/implementation-status.md` — cited; `website/docs/guides/configure-model-routes.md:57-61` ("all seven fields"), `website/docs/reference/formats.md:81-84` — inferred
- **Not expected to change:** the three clients' `src/`; each `Model::capabilities()` returns `self.binding.capabilities()` (`crates/llm-responses/src/client.rs:308`, `crates/llm-messages/src/client.rs:201`, `crates/llm-chat/src/client.rs:49`) — inferred
- **Confidence:** high for core, spec, generated files and compile-forced sites; medium for where the protocol refusal lands and whether the digests move
- **Would collide with:** any unit editing `spec/` (all of `contracts/schema` and `contracts/suite.json` regenerate), touching `Capabilities`, `BindingDocument::bind` or `Routes::parse`, or editing the test fixtures above — inferred
- **Safety fact:** `binding_revision` (`crates/llm-providers/src/binding.rs:86-97`) and `config_digest` (`crates/llm-routing/src/catalog.rs:176-194`) are SHA-256 over `serde_json` output that includes `Capabilities`. Written only when they differ from the default (`skip_serializing_if`), the new fields leave a pre-change document's hash unchanged; always written, every document gets a new revision on upgrade and opaque state a consumer stored earlier is refused as cross-binding — cited, unproven
- **Outside this repository:** adding public fields breaks every `Capabilities { … }` literal in a consumer — inferred; consumers not read

## Open for the implementor

- No protocol in scope carries input during output, so the refusal is "refuse `true` on every
  protocol in this workspace".
- A document declaring `streaming: false` for one of the three clients would be untrue; refuse it or
  accept it as a caller's claim. Record the choice in `docs/design.md`.
- Unrelated, found while scoping: `website/docs/concepts/routing.md:31-39` shows a catalog example
  without four capability fields, and `Capabilities` has no serde defaults at 0.7.0, so the example
  appears not to parse (not run).

## Design decisions

Taken 2026-10-20 from `docs/plan/wave-2026-10-20-w09-design-review.md` § Layer 1 (1.1 to 1.6).

- **Shape:** two public bools, `output_streaming` and `input_during_output`, the two claims the
  inference node reads (`Capability::OutputStreaming`, `Capability::ConcurrentInputOutput`).
- **Serialization:** serde default (`output_streaming` true, `input_during_output` false) and
  `skip_serializing_if` when equal to the default, so a document written before the change
  serializes to the same bytes and `binding_revision` and `config_digest` do not move. Precedents:
  `crates/llm-core/src/error.rs:47`, `crates/llm-core/src/turn.rs:89`.
- **Refusal:** one llm-core function taking the `Protocol`, called from `BindingDocument::bind` and
  from `Routes::parse` (`crates/llm-models/src/routes.rs:730-732`), so a routes refusal names the
  route. It refuses `input_during_output: true` ("no protocol in this workspace carries input during
  output") and `output_streaming: false` ("every protocol in this workspace streams its output").
  `Capabilities::validate` does not check either; a Model built in code keeps its own claim.
- **Consumer break:** plain public fields, plus additive `with_output_streaming` and
  `with_input_during_output` setters; `#[non_exhaustive]` is not added. The CHANGELOG names the 6
  full struct literals in consumers and points them to `..Capabilities::text(…)`.
- **Corrections:** the string-route default (`routes.rs:72-73`, `spec/domains/routes.yaml:65`,
  "every boolean true") is stated per field; `Capabilities::text`'s doc (`vocabulary.rs:61`) says it
  sets `output_streaming` true; `website/docs/guides/configure-model-routes.md:61` ("all seven
  fields") is updated; the field docs say `input_during_output` is false for every protocol here
  until story:input-into-a-running-turn decides otherwise.
