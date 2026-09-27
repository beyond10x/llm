---
format: aep.planning-md/2
id: story:chat-projection
kind: story
status: implemented
title: Chat Completions projects the supported neutral subset
relations:
- decomposes: epic:inference
- depends_on: story:http-streaming
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/chat.rs
- confidence: cited
  path: contracts/chat/scenarios
- confidence: cited
  path: crates/llm-chat
- confidence: inferred
  path: docs/chat.md
- confidence: inferred
  path: docs/verification/chat-falsification.json
- confidence: inferred
  path: docs/verification/chat-report.json
- confidence: inferred
  path: docs/verification/chat.md
- confidence: cited
  path: spec/domains/chat.yaml
revision: 9
---
## Context

Outgoing and ingress codecs cover the published subset; missing usage remains absent and unsupported features fail by name. Provide a vLLM-compatible fixture, not a fabricated live proof.

## Acceptance

Pinned Chat Completions fixtures preserve streamed text, tool arguments, usage and finish reasons through the neutral interface.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

Derived 2026-09-20 by `story-scoper`. Every line is **cited** (read from the artifact body, a diff,
or a file opened in the tree) or **inferred** (a reading that could be wrong).

- cited: `crates/llm-chat` — the artifact's own scope names it; the crate exists as a scaffold whose
  `src/lib.rs` states "exports no runtime API yet" and whose `Cargo.toml` declares no dependencies.
  The outgoing and ingress Chat Completions codecs, the pinned fixtures and the vLLM-compatible
  local HTTP fixture all land here, alongside the crate's own `Cargo.toml` dependency block (the
  shape `crates/llm-messages/Cargo.toml` took: `llm-core`, `llm-http`, `llm-providers`,
  `llm-credentials`, `serde`, `serde_json`, `http`, `tokio`, all already declared as workspace
  dependencies).
- cited: `contracts/chat` — the artifact's own scope names it. Authored scenarios go in
  `contracts/chat/scenarios/*.yaml`, one `type: ess-scenario/1` document per case, matching
  `contracts/inference/scenarios/` and `contracts/routing/scenarios/`.
- cited: `spec/domains/chat.yaml` — the file exists and currently holds one line, `domain: llm.chat`.
  This unit fills in the `entities`, `commands`, `events` and `views` for the domain, as
  `spec/domains/messages.yaml` does for `llm.messages`.
- cited: `checks/conformance/src/chat.rs` — the file exists as a placeholder whose doc comment reads
  "The unit implementing story:chat-projection replaces this file. Keep the two public items:
  `VIEWS` names every view this domain answers, and `observe` returns `None` for a command another
  domain owns. `target.rs` is shared and is not this unit's to edit."
- inferred: `docs/verification/chat.md` — every implemented domain publishes a verification note
  under `docs/verification/`. The exact stem varies across domains (`routing-conformance.md`,
  `budgets.md`, `pricing.md`), so the filename is a guess even though the surface is not.
- inferred: `docs/verification/chat-report.json` — the report paired with the suite, per
  `observations-report.json`, `pricing-report.json`, `routing-report.json`.
- inferred: `docs/verification/chat-falsification.json` — the falsification record that accompanies
  each report in that directory.
- inferred: `docs/chat.md` — the weakest line here. `docs/budgets.md`, `docs/pricing.md` and
  `docs/local-secrets.md` exist, but routing shipped with no `docs/routing.md` at all, so a
  protocol-level prose document may not be wanted. The sibling projection stories carry the
  equivalent entry.

**Confidence: high.** The artifact body names two of the four code surfaces directly, and the other
two exist in the tree as placeholders that name this story.

The shared integration surfaces this story would otherwise have touched are **already landed** and
are not this unit's to edit: `spec/system.yaml` already lists `llm.chat`;
`checks/conformance/src/main.rs` already declares `mod chat;`; `checks/conformance/Cargo.toml`
already depends on `b10x-llm-chat`; and `checks/conformance/src/target.rs` has been refactored so
each domain module returns an `Observed` value and `execute_command` dispatches through
`crate::chat::observe`, explicitly "so a new domain is a new file rather than an edit to this shared
one". Generated and coordinator-owned artefacts — `contracts/ess-inputs.yaml`, `contracts/suite.json`,
`contracts/schema/**`, `contracts/baseline.json`, `Cargo.lock`, `docs/implementation-status.md` —
change as a consequence of this work and are regenerated centrally, not edited here.

**Would collide with:** nothing else in wave 1, on the condition that every unit branches from a
commit carrying the pre-landed conformance scaffolding. Every surface above is exclusive to the
`chat` domain. The one unassessed risk is `crates/llm-core`: if the declared Chat subset needs a
neutral-vocabulary change there, that surface is shared with the Messages and Responses projections.
