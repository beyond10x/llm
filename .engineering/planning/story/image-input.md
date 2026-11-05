---
format: aep.planning-md/3
id: story:image-input
kind: story
status: active
title: A user turn carries an image on every wire
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:core-types-non-exhaustive
scope:
- confidence: inferred
  path: crates/llm-bedrock/src/converse.rs
- confidence: inferred
  path: crates/llm-chat/src/ingress.rs
- confidence: inferred
  path: crates/llm-chat/src/outgoing.rs
- confidence: inferred
  path: crates/llm-core/src/item.rs
- confidence: inferred
  path: crates/llm-core/src/turn.rs
- confidence: inferred
  path: crates/llm-core/src/vocabulary.rs
- confidence: inferred
  path: crates/llm-messages/src/codec.rs
- confidence: inferred
  path: crates/llm-messages/src/ingress.rs
- confidence: inferred
  path: crates/llm-responses/src/ingress.rs
- confidence: inferred
  path: crates/llm-responses/src/request.rs
- confidence: inferred
  path: crates/llm-routing/src/selection.rs
- confidence: inferred
  path: docs/contract-v1.md
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/bedrock.yaml
- confidence: inferred
  path: spec/domains/chat.yaml
- confidence: inferred
  path: spec/domains/inference.yaml
- confidence: inferred
  path: spec/domains/messages.yaml
- confidence: inferred
  path: spec/domains/responses.yaml
revision: 23
transitions:
- {from: "draft", to: "proposed", at: "2026-11-04T07:35:28Z", actor: "human:timo", revision: 20}
- {from: "proposed", to: "active", at: "2026-11-04T07:35:28Z", actor: "human:timo", revision: 21}
---
## Context

A downstream adopter reading llm 0.20.0 reported that a turn cannot carry an image. Verified against
the code on 2026-11-04:

- `Item` (`crates/llm-core/src/item.rs:17`) has `UserText` and no image item.
- `docs/contract-v1.md:4`: "Image/audio ... are outside this version and must be refused by adapters".
- Responses refuses image content on either side (`docs/responses.md:226`) and reads exactly one text
  part per message (`crates/llm-responses/src/request.rs:848`).
- Messages refuses image blocks (`docs/messages.md:109`).

Every wire llm speaks has a documented image input:

- OpenAI Responses `input_image` (schema `InputImageContent`) and Chat Completions `image_url` parts
  (schema `ChatCompletionRequestMessageContentPartImage`), `openai/openai-openapi` `openapi.yaml`.
- Anthropic Messages `image` content blocks with a `base64` or `url` source.
- Bedrock Converse `image` content blocks `{format, source: {bytes}}`
  (https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html).

## Outcome

A caller sends an image in a user turn, and every wire carries it or refuses it before I/O.

## Acceptance

- The neutral turn has a user image item: a media type (`image/png`, `image/jpeg`, `image/gif`,
  `image/webp`) and either inline bytes or an `https` URL. A turn without one is the 0.20.0 turn
  exactly.
- Responses, Chat, Messages and Bedrock Converse send it in the same user message as the adjacent
  user text, in item order; each is a recorded fixture body. A wire that cannot carry the source
  (Converse and a URL) refuses by name before I/O.
- Ingress decoders read image parts in a user message into image items in order, so a message of
  text and image parts is no longer refused; an image in an assistant or tool message stays refused
  by its path.
- Inline bytes have a size bound refused before I/O; no image byte or URL reaches a diagnostic or a
  `Debug` form.
- The catalog capability and the selection rejection for image input are
  `story:output-and-image-capabilities`, which follows this story in the same wave.
- Image tokens are reported as the far side counts them; an absent counter stays unknown.
- `docs/contract-v1.md` and `docs/design.md` move image input inside the contract; audio stays out.
- ESS: the `llm.inference` domain and the four protocol domains declare the item; authored
  scenarios per wire; the conformance baseline rises and skips none.

## Verification

Targeted tests per touched crate, fixture bodies per wire, conformance scenarios. No live call.

## Schedule

Ships in the breaking minor 0.22.0, wave 2026-11-05-w26 (`approval-record:image-input-breaking-minor`,
`approval-record:wave-2026-11-05-w26-proposal`), after `story:core-types-non-exhaustive`.
