---
format: aep.planning-md/3
id: story:messages-input-total-with-an-absent-cache-counter
kind: story
status: archived
title: A Messages input total is known when an omitted cache counter is documented as zero
summary: llm 0.7.0 leaves the Messages input total unknown when cache_creation_input_tokens is absent; Loom's metering on llm's Model needs the total
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-messages/src/usage.rs
- confidence: cited
  path: crates/llm-messages/tests/adversary_streaming.rs
- confidence: inferred
  path: crates/llm-messages/tests/streaming.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: docs/messages.md
- confidence: inferred
  path: spec/domains/messages.yaml
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T02:49:02Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T02:49:03Z", actor: "human:timo", revision: 6}
- {from: "active", to: "archived", at: "2026-10-20T02:50:28Z", actor: "human:timo", revision: 8}
---
## Context

Reported by the Harness side of the cutover (parity row M25 of `docs/harness-parity.md`) through
session `adr-changes` on 2026-10-20. At tag 0.7.0 llm's Messages decoder computes the inclusive
input total only when all three components are reported, and leaves it unknown otherwise
(`Snapshot::normalized`, `crates/llm-messages/src/usage.rs:85-102`, pinned by
`each_component_of_the_inclusive_input_absent_in_turn_leaves_it_unknown`,
`crates/llm-messages/tests/adversary_streaming.rs:137`). When a Messages usage report has no
`cache_creation_input_tokens`, the total is unknown. Harness's own wire treated an absent cache
write as absent and a missing cache read as 0 (`harness-messages/src/project.rs:400` at Harness
`2fd7235b`).

Loom's metering moves onto llm's `Model` (Atlas ADR 0098 point 4) and needs the total.

The invariant "Unknown usage stays unknown, never zero" (AGENTS.md) stands: an absent counter
becomes 0 only where the provider documents that the absence means zero, and then it is a reported
zero, not a guess.

## Outcome

A Messages turn whose usage report omits a cache counter has a known input total exactly when the
provider documents that the omitted counter means zero; otherwise it stays unknown, and the caller
can tell which.

## Acceptance

- `docs/messages.md` cites the provider documentation for each of `cache_creation_input_tokens` and
  `cache_read_input_tokens` and states what an absent field means.
- Where the documentation says absent means zero: a test that a report without that field yields
  a known total equal to the reported parts; where it does not: the existing unknown-total tests
  stay as they are.
- `crates/llm-messages/tests/adversary_streaming.rs:137` changes only with a recorded reason in
  `CHANGELOG.md`; M25 in `docs/harness-parity.md` is updated.

## Design decisions

Taken 2026-10-20 by session `adr-changes` under the owner's grant
(`approval-record:wave-2026-10-20-w09-messages-input-total`).

- An absent `cache_creation_input_tokens` in a Messages usage report counts as 0, as the vendor
  does. `docs/messages.md` cites the vendor documentation (or, if the documentation is silent, the
  vendor's own SDK or client code) that treats it so; the citation is a requirement, not decoration.
- Only that counter. An absent `input_tokens`, `output_tokens` or `cache_read_input_tokens` stays
  unknown, and a total still needs every other component reported.
- The exception is written where the rule is: AGENTS.md's invariant row "Unknown usage stays
  unknown, never zero" names it (the coordinator's file), and `docs/design.md` records it with the
  reason (Harness parity row M25; Loom's metering, Atlas ADR 0098 point 4).
- `each_component_of_the_inclusive_input_absent_in_turn_leaves_it_unknown`
  (`crates/llm-messages/tests/adversary_streaming.rs:137`) changes for the cache-creation component
  only, and `CHANGELOG.md` records why.

## Archived

Archived 2026-10-20 before any work started: its rule ("an absent `cache_creation_input_tokens` counts as 0") was retracted by `adr-changes`, because it breaks the AGENTS.md rule "Unknown usage stays unknown, never zero". Superseded by `story:messages-input-total-derived-from-what-llm-observed`, which records why the derivation adr-changes proposed instead does not reach the reported case.
