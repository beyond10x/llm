---
format: aep.planning-md/3
id: story:responses-carries-refusal-text
kind: story
status: active
title: A Responses binding carries refusal text both ways
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- informed_by: review-result:adversary-w24-chat-deltas-pass-1
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/responses/scenarios
- confidence: inferred
  path: contracts/routing/scenarios
- confidence: cited
  path: contracts/suite.json
- confidence: inferred
  path: crates/llm-core/src/item.rs
- confidence: inferred
  path: crates/llm-core/src/port.rs
- confidence: inferred
  path: crates/llm-models/tests/adversary_w24_refusal_route.rs
- confidence: cited
  path: crates/llm-responses/src/ingress.rs
- confidence: cited
  path: crates/llm-responses/src/request.rs
- confidence: cited
  path: crates/llm-responses/src/stream.rs
- confidence: cited
  path: crates/llm-responses/tests
- confidence: cited
  path: crates/llm-routing/src/selection.rs
- confidence: cited
  path: crates/llm-routing/tests
- confidence: cited
  path: docs/chat.md
- confidence: inferred
  path: docs/design.md
- confidence: cited
  path: docs/implementation-status.md
- confidence: cited
  path: docs/responses.md
- confidence: inferred
  path: docs/verification/responses-falsification.json
- confidence: inferred
  path: docs/verification/responses-report.json
- confidence: inferred
  path: docs/verification/responses.md
- confidence: inferred
  path: spec/domains/responses.yaml
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/guides/use-the-llm-command.md
- confidence: inferred
  path: website/docs/status.mdx
revision: 31
transitions:
- {from: "draft", to: "proposed", at: "2026-11-25T14:06:41Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-25T14:06:41Z", actor: "human:operator", revision: 3}
---
## Context

Wave 2026-10-30-w24 (`story:chat-counts-every-output-delta`) added `Item::AssistantRefusal` and
`StreamEvent::RefusalDelta`. Only the Chat wire carries them; the Responses projection and its
ingress encoder refuse them by name, and routing selection rejects a non-Chat target for a
conversation that carries refusal text (`Rejection::RefusalText`).

`review-result:adversary-w24-chat-deltas-pass-1` (finding at `docs/chat.md:94`) points out that the
Responses wire likely has a `refusal` content part on assistant output messages. Inferred from the
public API reference, not checked against a server or a recorded fixture in this repository.

## Outcome

A Responses binding carries refusal text both ways, so a conversation holding a refusal can be
routed to a Responses target.

## Acceptance

- A Responses stream whose output message holds a `refusal` content part reaches the caller as
  `StreamEvent::RefusalDelta` and `Item::AssistantRefusal`, never as assistant text.
- An `Item::AssistantRefusal` in a request is projected as that content part, and the Responses
  ingress encoder writes it.
- Selection no longer rejects a Responses target for refusal text.

## Scope

Derived 2026-11-25 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-responses/src` (`stream.rs`, `request.rs`, `ingress.rs`), with one condition in `crates/llm-routing/src/selection.rs` — cited (story, tree)

Acceptance 1, the answer decoder:
- `crates/llm-responses/src/stream.rs:949` `message_item`: reads an output message's `content`; a part other than `output_text`/`text` returns `Error::unsupported("message content is outside the pinned subset")` with dispatch `accepted` (`:963-966`). A `refusal` part is therefore a failed turn today, not assistant text and not an item — cited (tree)
- There is one decoder for both paths: `decode_stream` (`stream.rs:94`). The streamed path reaches `message_item` from `response.output_item.done` in `Decoder::apply` (`:350`, `:386-404`, error deferred to the terminal object); the whole-answer path reaches it from the terminal object's `output` in `Decoder::terminate` (`:636`, `:663-682`) through `output_item` (`:821`). The client is stream-only (`request.rs:179` writes `stream: true`, `:336` requires it) — cited (tree)
- `stream.rs:35-61` `ACCEPTED_STREAM_EVENTS` holds no `response.refusal.*` name, so such an event falls to the `_` arm of `apply` (`:418-431`): `Warning{code: "unknown-stream-event"}` plus an `Item::Opaque` kept — cited (tree)
- New: `response.refusal.delta` becomes `StreamEvent::RefusalDelta`, a `refusal` part becomes `Item::AssistantRefusal`; `message_item` returns one `Item` today and a message holding both an `output_text` and a `refusal` part needs two — inferred
- The kept-text machinery is keyed to `Item::AssistantText` only: `show` (`:479`), `end_message` (`:512`), `keep_shown_text` (`:552`), `adopt_streamed` (`:296`), the `matches!(.., Item::AssistantText)` at `:393` and `:670`. Refusal deltas shown to the caller fall under the invariant "a streamed delta is final" (`AGENTS.md:67`), so this code likely grows a refusal branch — inferred

Acceptance 2, the request projection and the ingress encoder:
- `crates/llm-responses/src/request.rs:748` `item_to_input`: `Item::AssistantRefusal` returns `Error::unsupported("the Responses projection carries no assistant refusal text")` (`:753-758`). It is refused, not dropped. `message` (`:617`) builds the one-part message the new arm would use — cited (story, tree)
- `crates/llm-responses/src/ingress.rs:564` `check_outcome`: `Item::AssistantRefusal` returns `Error::unsupported("the Responses ingress encoder carries no assistant refusal text")` (`:605-610`). `IngressStream::chunk` (`:184`) sends nothing for `StreamEvent::RefusalDelta` (`:271-276`); `close` (`:301`) skips the item (`:392`); `wire_item` (`:771`) maps it to `Value::Null` (`:786`); `message_item` (`:799`) writes only an `output_text` part; `check_streamed` (`:498`) pairs shown text — cited (story, tree)
- `crates/llm-responses/src/request.rs:858` `input_to_item` (the ingress decoder, under `ingest_request` `:323`): an assistant message is read through `content_text` (`:992`) and `part_text` (`:1098`) as exactly one `output_text` part; a `refusal` part is refused `unsupported`. The acceptance does not name this reader. The crate's rule that one contract serves both directions (`src/lib.rs:6-12`, `docs/responses.md:345-349`) and the scenario field `reprojected` make it part of the change — inferred

Acceptance 3, selection:
- `crates/llm-routing/src/selection.rs:275` `capability_rejections`, the condition at `:349-358`: every target whose protocol is not `Protocol::ChatCompletions` gets `Rejection::RefusalText` (`:80`), label `refusal-text` (`:97`), for a request holding `Item::AssistantRefusal` — cited (story, tree)
- The variant stays: Messages and Bedrock Converse still refuse the item (below), so the condition becomes "neither Chat Completions nor Responses". `spec/domains/catalog.yaml:55` (`llm.catalog.RouteRejection`) and `contracts/schema/schema/types/llm.catalog.Route{Rejection,Explanation}.schema.json`, `llm.catalog.TargetExplanation.schema.json` keep their bytes — inferred

The shape the story repeats:
- Chat Completions carries it: `crates/llm-chat/src/incoming.rs` `delta` (`:175`, `RefusalDelta` at `:214-220`), `outcome` (`:314`, item at `:329-333`), `completion` (`:397`, `:423-427`); `outgoing.rs` `messages` (`:159`, `:198-204`, the message's own `refusal` key); `ingress.rs` `decode_messages` (`:188`, `:239-243`), `IngressStream::chunk` (`:759`, `:770-773`), `close` (`:829`, `:873`), `split_items` (`:943`, `:950`) — cited (tree)
- Messages does not carry it: `crates/llm-messages/src/codec.rs:44` `block` refuses at `:52-56`, `ingress.rs` `content` refuses at `:666-670`, its stream encoder sends nothing for `RefusalDelta` (`:222`). Bedrock Converse refuses in `crates/llm-bedrock/src/converse.rs` `content_block` (`:224-228`). Both stay as they are — cited (tree)

Public types:
- No type, variant or signature changes in `llm-core`, `llm-responses` or `llm-routing`: `Item::AssistantRefusal` (`crates/llm-core/src/item.rs:77`), `StreamEvent::RefusalDelta` (`crates/llm-core/src/port.rs:62`) and `Rejection::RefusalText` exist. The value of the public constant `llm_responses::ACCEPTED_STREAM_EVENTS` grows — inferred
- Doc comments that say only Chat Completions carries it: `crates/llm-core/src/item.rs:74-75`, `crates/llm-core/src/port.rs:58-61`, `crates/llm-routing/src/selection.rs:349-350` — inferred
- Behaviour a library caller sees: a Responses turn answered with a `refusal` part returned `Err(unsupported)` and returns `Ok` holding `Item::AssistantRefusal`; a request holding the item was refused before I/O and is sent; `Catalog::explain` stops listing `refusal-text` for a Responses target, so a route with fallback can move such a conversation to one. Nothing stops compiling. Whether the CHANGELOG line starts `**Breaking:**` is the owner's reading of "breaking" for a failure that becomes a returned turn — inferred

Tests that hold today's behaviour and change:
- `crates/llm-responses/tests/ingress.rs:1009` `refusal_text_is_refused_by_name_and_never_dropped` (encoder and projection, both fixed messages) — cited (tree)
- `crates/llm-responses/tests/adversary.rs:142` `a_content_part_outside_the_pinned_subset_is_not_dropped_without_a_word` and `crates/llm-responses/tests/projection.rs:1333-1339` (a case of `every_stream_refusal_the_contract_promises_is_reachable`, `:1271`): both use a `refusal` part as their example of unsupported content and need another part type — cited (tree)
- `crates/llm-routing/tests/catalog.rs:86` `refusal_text_rejects_every_target_whose_wire_cannot_send_it`: its rejected target is the Responses secondary of `examples/catalog.toml`; the rejection needs a Messages or Converse target to stay asserted — cited (tree)
- `crates/llm-models/tests/adversary_w24_refusal_route.rs`: the case keeps passing only when the projection carries the item in the same change as selection admits the target; its header comment (`:3-8`) goes stale — inferred
- `crates/llm-routing/tests/adversary_w26_feature_selection.rs:470` lists the variant and does not change — cited (tree)
- New cases for the three acceptance lines land in `crates/llm-responses/tests/` and `crates/llm-routing/tests/` — inferred
- Recorded fixtures: `crates/llm-responses/tests/fixtures/` (`codex/`, `live/`, five request files) holds no refusal, and no file in the repository contains `response.refusal`. The only refusal part in the tree is the hand-built `{"type":"refusal","refusal":"I cannot help with that."}` in the two tests above and one scenario. The tests build their own payloads — cited (tree)

ESS:
- `spec/domains/responses.yaml`: no entity, command, event or view names refusal text or the capability. `StreamDecoding.items_json` and `stream_events` (`:215-241`), `RequestProjection`, `RequestIngestion` and `IngressEncoding` carry it as strings, and the four commands (`Project` `:314`, `DecodeStream` `:338`, `Ingest` `:353`, `EncodeIngress` `:371`) take raw JSON. No field has to change — cited (tree)
- The conformance target already renders both kinds: `checks/conformance/src/responses.rs:475` (`assistant-refusal`), `:495` (`refusal-delta:`). No change there — cited (tree)
- A rule paragraph in the header comment of `spec/domains/responses.yaml` (its form: "Kept text", "Named refusal cause", `:1-175`) — inferred
- One scenario holds today's rejection: `contracts/responses/scenarios/message-content-outside-the-subset-is-refused-not-emptied.yaml` (a `refusal` part, `accepted: false`, `error_code: unsupported`), listed at `contracts/ess-inputs.yaml:343`. It changes its part type or goes — cited (tree)
- No scenario holds the projection's refusal, the encoder's refusal or the `refusal-text` rejection: no file under `contracts/` contains `assistant-refusal` or `refusal-text` — cited (tree)
- New scenarios: `contracts/responses/scenarios/` (decode streamed and from the terminal object, project, ingest, encode streamed and whole) and `contracts/routing/scenarios/` (`llm.routing.Evaluate`, `spec/domains/routing.yaml:126`, field `rejections` `:83`: a Responses target is not rejected and a Messages target still is) — inferred
- With them move `contracts/ess-inputs.yaml` (one line per scenario), `contracts/baseline.json:2-3` (`answered_floor`, `total_floor`, 1090 today: 1046 authored and 44 generated), `AGENTS.md:80` and a "the floor is N (was 1090)" line in `CHANGELOG.md` — inferred
- `contracts/suite.json` is generated from the scenarios (`AGENTS.md:195`) and is regenerated because one existing scenario changes; its `provenance` holds a `spec_digest` and a `contract_digest` — cited (tree)
- `contracts/schema/` moves only if `spec/` changes a declaration; a comment alone is expected to leave its bytes — inferred

- **Documents:** `docs/responses.md`: items table (`:158-166`), the refusal table under `:238`, the stream table (`:304-315`), the paragraph that names a `refusal` part as refused (`:351-356`), the encoder's "refused" table under `:476` — cited (tree)
- `docs/chat.md:101-107`: "Responses has a refusal content part that this repository does not carry yet (`story:responses-carries-refusal-text`)" and "selection rejects every target other than Chat Completions" — cited (story, tree)
- `docs/implementation-status.md:101`: the `chat-counts-every-output-delta` row says "routing rejects non-Chat targets" — cited (tree)
- A new row under `### Protocols` (`docs/implementation-status.md:77`), then the generated `website/data/status.json` and `website/docs/status.mdx` (`AGENTS.md:194`) — inferred
- `website/docs/guides/use-the-llm-command.md:122-123` ("Chat Completions sends a refusal apart from the text") — inferred
- `docs/design.md`: no line names refusal text on Responses today (`:1300-1306` is the Responses ingress bullet, `:1308` the Chat one); a sentence there if the ingress decoder reads the part — inferred
- `AGENTS.md` invariants table: rows `:67` (a streamed delta is final) and `:79` (no field a client sends is dropped) gain a test path if refusal text joins them; `:80` is the floor — inferred
- `CHANGELOG.md`: lines under `## [Unreleased]`, which today holds only `### Fixed` (`:9`) — inferred
- `README.md` has no line on refusal text or on what each wire carries — cited (tree)
- `docs/verification/responses.md:81`, `docs/verification/responses-falsification.json:212,352-359` and `docs/verification/responses-report.json:94` name the scenario and the Rust case above; they are records of past runs and go stale if either is renamed — inferred

- **Confidence:** medium — every site that refuses today is read from the tree; the wire shape (`response.refusal.delta`, `response.refusal.done`, the `refusal` part) is in no fixture and no server answer here, as the story itself says, and how far the kept-text code and the ingress decoder move is a reading — inferred

- **Would collide with:** any unit that edits `crates/llm-responses/src/stream.rs`, `request.rs` or `ingress.rs`; any unit that adds, edits or moves a file under `crates/llm-responses/tests` or `crates/llm-routing/tests`; any unit that edits `crates/llm-routing/src/selection.rs`; any unit that adds or removes an authored scenario (`contracts/suite.json`, `contracts/baseline.json:2-3`, `contracts/ess-inputs.yaml`, `AGENTS.md:80`, the floor line in `CHANGELOG.md`); any unit that edits `docs/responses.md`, `docs/chat.md:90-107`, `docs/implementation-status.md` or `website/docs/guides/use-the-llm-command.md` — inferred
- Overlap with `story:test-binaries-one-per-crate`: its part 2 moves all 43 files of `crates/llm-responses/tests`; this story edits `ingress.rs`, `adversary.rs` and `projection.rs` there and `catalog.rs` in `crates/llm-routing/tests`. A case edited in a file that is moved conflicts; both also rewrite test paths in the `AGENTS.md` invariants table and touch `docs/responses.md`, `docs/implementation-status.md`, `website/data/status.json` and `website/docs/guides/use-the-llm-command.md` — cited (that story's scope entries, tree)
- Overlap with `story:tool-arguments-with-a-repeated-key`: the same file `crates/llm-responses/src/stream.rs`, other functions (`function_call` `:840-870` there; `apply`, `output_item`, `message_item` and the constant here); `crates/llm-responses/tests`; `contracts/responses/scenarios` (other files); the shared contract files and floor; `docs/responses.md` (its stream table) and `docs/chat.md:90-95`, next to this story's `:101-107`; `crates/llm-core/src/item.rs` if this story edits the doc comment — cited (that story's Scope, tree)
- Overlap with `story:replay-stops-at-the-last-case-have-scenarios`: no source or test file; `contracts/suite.json` (regenerated after the merge, never merged by hand), `contracts/baseline.json:2-3`, `contracts/ess-inputs.yaml`, `AGENTS.md:80` and the Unreleased block of `CHANGELOG.md` — cited (that story's Scope)
- Overlap with `story:cli-route-explain`: it has no Scope section. It prints `Rejection::label()` values from `Catalog::explain`; `refusal-text` keeps its name, and after this story a Responses target no longer shows it. Its acceptance names flags for structured output, image input and tools, not refusal text. Shared files are likely `website/docs/guides/use-the-llm-command.md`, `docs/implementation-status.md`, `website/data/status.json`, `README.md` and `CHANGELOG.md`; `crates/llm-cli/src/lib.rs` already prints both kinds (`:891`, `:1107`) and does not change here — inferred

- **Split:** one unit is enough. If it is split, the parts are ordered, not parallel: (A) `crates/llm-responses/src`, `crates/llm-responses/tests`, `contracts/responses/scenarios`, `spec/domains/responses.yaml`, `docs/responses.md`; then (B) `crates/llm-routing/src/selection.rs`, `crates/llm-routing/tests/catalog.rs`, `crates/llm-models/tests/adversary_w24_refusal_route.rs`, `contracts/routing/scenarios`, `docs/chat.md`, `docs/implementation-status.md`, the site pages. B before A makes selection admit a target whose projection still refuses, the defect `crates/llm-models/tests/adversary_w24_refusal_route.rs` was written for. Both parts move `contracts/suite.json`, `contracts/baseline.json`, `contracts/ess-inputs.yaml`, `AGENTS.md:80` and `CHANGELOG.md`, so those are not disjoint. Inside A the decoder comes before the encoder, because the encoder's tests and the `EncodeIngress` scenarios read every answer back through `decode_stream` — inferred

- **Safety fact:** every consumer downstream of the Responses decoder already handles both kinds, so producing them adds no new case to any match: `ShownText` (`crates/llm-core/src/port.rs:323`, `:374`), turn validation (`crates/llm-core/src/turn.rs:155`, `:247`, `:443`), `llm infer` (`crates/llm-cli/src/lib.rs:891`, `:1107`), `llm-decide` (`crates/llm-decide/src/answer.rs:72`, a refusal beside the answer abstains) and the conformance target (`checks/conformance/src/responses.rs:475`, `:495`) — step 2, unproven. To prove: the wire shape against a server or a recorded answer; that `Rejection::RefusalText` still fires for Messages and Converse; that `call_tool` (`crates/llm-tool-call/src/lib.rs`, which names neither kind) ends a refused Responses turn as a failure and not as a missing call; that a route falling back from Chat Completions to Responses with a refusal in the conversation is wanted — inferred
