---
format: aep.planning-md/3
id: story:embed-route-kind
kind: story
status: implemented
title: 'An embed route kind: embed(texts) returns vectors with token usage, a timeout and batches'
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:call-records
scope:
- confidence: inferred
  path: .engineering/specs/llm-seams/domains/embeddings.yaml
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: checks/conformance/Cargo.toml
- confidence: inferred
  path: checks/conformance/src/embeddings.rs
- confidence: inferred
  path: checks/conformance/src/target.rs
- confidence: inferred
  path: contracts/embeddings/scenarios
- confidence: inferred
  path: crates/llm-core/src/lib.rs
- confidence: inferred
  path: crates/llm-core/src/port.rs
- confidence: inferred
  path: crates/llm-embed/Cargo.toml
- confidence: inferred
  path: crates/llm-embed/src/lib.rs
- confidence: inferred
  path: crates/llm-embed/tests/embed.rs
- confidence: inferred
  path: crates/llm-routing/src/catalog.rs
- confidence: inferred
  path: crates/llm-routing/src/selection.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: spec/domains/catalog.yaml
- confidence: inferred
  path: spec/domains/embeddings.yaml
- confidence: inferred
  path: spec/system.yaml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T18:17:59Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-05T18:17:59Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-06T10:56:57Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

A downstream adopter needs `embed(texts) -> vectors` with token usage and a timeout. Verified at
llm 0.22.0: no beyond10x crate offers it (grep of llm, llm-gateway, harness, metaharness, bench,
tools, loom, connectors, epistemic-knowledge-runtime); inference lists embeddings out of scope.
Routes and protocols in llm are turn-shaped.

## Outcome

An embed route kind: `llm.catalog.Route` gains an optional `kind` (`turn`, `embed`; absent is
`turn`, not written, so an older catalog keeps its bytes and digest). An `Embedder` port in
`llm-core` takes texts and a timeout and returns one vector per text in input order, with token
usage (unknown stays unknown). The first wire is OpenAI `POST /v1/embeddings` (OpenAI and
OpenAI-compatible servers) over `llm-http`, in a new crate `llm-embed`. A request larger than the
target's declared batch limit is split in order and reassembled in input order. A turn to an embed
route, or an embed request to a turn route, is refused before I/O as `unsupported`. Each batch
yields an `llm.calls.CallRecord` of kind `EmbedBatch`.

## Specification

Domain `llm.embeddings`, drafted at `.engineering/specs/llm-seams/domains/embeddings.yaml`
(`EmbedRequest`, `EmbedBatch`, `Embedding`, command `llm.embeddings.Embed`), validated 2026-11-05
with ess 0.86.0. This story moves it to `spec/domains/embeddings.yaml`, adds it to
`spec/system.yaml`, and adds `kind` to `llm.catalog.Route` in `spec/domains/catalog.yaml`.

## Acceptance

Scenarios in `contracts/embeddings/scenarios/`, listed in `contracts/ess-inputs.yaml`, answered:

- `embed-returns-one-vector-per-text-in-input-order`
- `a-request-over-the-batch-limit-is-split-and-returned-in-input-order`
- `an-unreported-token-count-stays-unknown`
- `an-embed-request-past-its-timeout-fails-deadline`
- `a-turn-to-an-embed-route-is-refused-before-io`
- `an-embed-request-to-a-turn-route-is-refused-before-io`
- `a-catalog-without-route-kind-keeps-its-digest`
- `each-batch-yields-a-call-record-of-kind-embed-batch`

And: `docs/design.md` gains an "Embeddings" section; the baseline rises, skips none.

## Not in scope

Bedrock embeddings and other wires: a later story each. Vector storage and retrieval: the
caller's.

## Verification

Targeted: `-p llm-embed` named test targets, `-p llm-routing --test catalog`, the conformance
check. No live call.
