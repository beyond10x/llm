---
format: aep.planning-md/1
id: story:responses-projection
kind: story
status: active
title: Responses projects the supported neutral subset
relations:
- decomposes: epic:inference
- depends_on: story:http-streaming
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/responses.rs
- confidence: cited
  path: contracts/responses/scenarios
- confidence: cited
  path: crates/llm-responses
- confidence: inferred
  path: docs/responses.md
- confidence: inferred
  path: docs/verification/responses-falsification.json
- confidence: inferred
  path: docs/verification/responses-report.json
- confidence: inferred
  path: docs/verification/responses.md
- confidence: cited
  path: spec/domains/responses.yaml
revision: 8
---
## Context

Support both outgoing calls and gateway ingress projection using one protocol contract. Retain unknown events/opaque items or refuse explicitly. Include existing Harness/vLLM reasoning-event evidence instead of claiming all Responses behavior.

## Acceptance

Pinned request and streaming fixtures round-trip the supported Responses text/tool subset and reject incompatible continuation state without losing usage or terminal truth.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

Derived 2026-09-20 by `story-scoper`. Every line is cited (read from the story body, a diff or a
file opened in the tree) or inferred (a reading of the established pattern that could be wrong).

- cited: `crates/llm-responses` — the body's scope names it; the crate exists as a scaffold
  (`src/lib.rs`, "exports no runtime API yet") and this story is the only one that fills it.
- cited: `spec/system.yaml` — named in Evidence and read: it enumerates `domains:` and the
  in-flight messages work appends `llm.messages` to it. A `llm.responses` domain must be added here.
- cited: `checks/conformance/src/target.rs` — read: `execute_command` dispatches on command name
  and `query_view` whitelists view names. A Responses adapter must add arms to both.
- cited: `checks/conformance/src/main.rs` — read: the `mod` list (`budgets`, `gate`, `inference`,
  `pricing`, `secrets`, `target`) must gain `mod responses;`.
- cited: `checks/conformance/Cargo.toml` — read: adapters take an explicit path dependency
  (`llm-routing`, `llm-cost`); the Responses adapter needs one on `b10x-llm-responses`.
- inferred: `contracts/responses/scenarios` — the body cites `contracts/responses`; every domain
  keeps authored ESS scenarios under `<domain>/scenarios/` (`contracts/inference/scenarios`), with
  request and SSE fixtures inline in the scenario YAML.
- inferred: `spec/domains/responses.yaml` — mirrors `spec/domains/messages.yaml`
  (`CodecInspection` entity, `Project` / `DecodeStream` commands, `LastInspection` view); the body
  cites no domain file of its own.
- inferred: `checks/conformance/src/responses.rs` — one adapter module per domain
  (`inference.rs`, `pricing.rs`, `secrets.rs`, `budgets.rs`) observing the production codec.
- inferred: `docs/responses.md` — the caller-facing contract page each domain carries
  (`docs/pricing.md`, `docs/budgets.md`, `docs/local-secrets.md`).
- inferred: `docs/verification/responses.md` — the verification record; its stem is topical, not
  domain-keyed (`observations.md` covers `llm.inference`), so the exact name may differ.
- inferred: `docs/verification/responses-report.json` — the report paired with the suite, per
  `observations-report.json`.
- inferred: `docs/verification/responses-falsification.json` — the falsification record, per
  `observations-falsification.json`.

Primary surface is `crates/llm-responses` plus its contract and verification files, which nothing
else touches. The four cited shared files — `spec/system.yaml`, `checks/conformance/src/main.rs`,
`checks/conformance/src/target.rs`, `checks/conformance/Cargo.toml` — are integration surfaces every
new projection domain must edit, and are the whole of this story's collision risk. Coordinator-owned
files this story forces changes to (`contracts/ess-inputs.yaml`, `contracts/suite.json`,
`contracts/schema/**`, `contracts/baseline.json`, `Cargo.lock`, `docs/implementation-status.md`) are
listed in the wave notes, not here. Confidence: high — the body cites crate and contract directory,
and five implemented domains show one uniform pattern read directly from the tree.
