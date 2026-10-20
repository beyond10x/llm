---
format: aep.planning-md/3
id: story:messages-input-total-derived-from-what-llm-observed
kind: story
status: implemented
title: A Messages input total is known whenever llm can derive it from what it observed
summary: llm 0.7.0 leaves the total unknown with cache_creation_input_tokens absent; deriving 0 from a request without cache_control rarely fires, because llm marks cache_control on nearly every request
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
- supersedes: story:messages-input-total-with-an-absent-cache-counter
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-messages/src/usage.rs
- confidence: inferred
  path: crates/llm-messages/tests
- confidence: inferred
  path: docs/harness-parity.md
- confidence: cited
  path: docs/messages.md
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T15:37:08Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T15:37:08Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-20T21:51:17Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

Reported from the Harness cutover (parity row M25) through session `adr-changes` on 2026-10-20: with
`cache_creation_input_tokens` absent from a Messages usage report, llm 0.7.0 leaves the inclusive
input total unknown (`Snapshot::normalized`, `crates/llm-messages/src/usage.rs:95`; pinned by
`each_component_of_the_inclusive_input_absent_in_turn_leaves_it_unknown`,
`crates/llm-messages/tests/adversary_streaming.rs:137`), so a Harness run through llm files no usage
or cost. Loom's metering on llm's `Model` (Atlas ADR 0098 point 4) needs the total too.

The rule "Unknown usage stays unknown, never zero" (AGENTS.md) stands. `adr-changes` retracted an
earlier "count it as 0" (2026-10-20) and asked for a derivation instead: when the request carried no
`cache_control`, no cache write can have happened, so cache creation is 0 by the request; with
`cache_control` present and the field absent the total stays unknown.

**Why that derivation does not reach the reported case.** llm's own Messages encoder marks
`cache_control` on every request that has instructions (`crates/llm-messages/src/codec.rs:163`)
and places a rolling breakpoint on the last markable block of the last message
(`codec.rs:88-104`). A turn sent through llm's `MessagesClient` therefore nearly always carries
`cache_control`, and the derived total would stay unknown for it. The derivation is small (one flag
from the client into the decoder) but would fire only for a request with no instructions and no
markable block.

## What has to be found out first

- The only upstream known to omit `cache_creation_input_tokens` is the Harness spike's own Messages
  emulator ("the spike's Messages emulator omits cache_creation_input_tokens", Harness's report
  relayed by session `adr-changes`, 2026-10-20). No observation of the Anthropic API or a
  compatible server omitting it is known.
- What Anthropic documents for the field when a request carries `cache_control` and no write
  happens (present as 0, or absent): not yet read.

## Outcome

The Messages input total is known whenever it can be derived from what llm observed, and the
derivation is one that fires for the runs that reported the gap.

## Acceptance

- The upstream that omits the field is named, with a captured response or its documentation.
- A derivation from observed facts (the request's `cache_control`, a documented field contract) is
  recorded in `docs/messages.md` and pinned by tests both ways: total known where derived, unknown
  where not.
- No absent counter is reported as zero by assumption; AGENTS.md's invariant row is unchanged.

## Design decisions

Approved with wave 2026-10-20-w11 by session `b10x-chef-orchestrator` under the owner's grant.

The documented contract: Anthropic's API reference (`platform.claude.com/docs/en/api/messages.md`,
fetched 2026-10-20, lines 4119-4141) says the total input is the sum of `input_tokens`,
`cache_creation_input_tokens` and `cache_read_input_tokens`; `cache_creation_input_tokens` is
"number or null"; `cache_creation` breaks it down by TTL into `ephemeral_1h_input_tokens` and
`ephemeral_5m_input_tokens`, each "default: 0". llm already reads that breakdown
(`crates/llm-messages/src/usage.rs:46-65` at 0.8.2).

The derivation: when `cache_creation_input_tokens` is absent and the `cache_creation` breakdown is
present with both counts, the creation count is their sum; otherwise the total stays unknown. The
only upstream known to omit the total is the Harness spike's Messages emulator; the unit reports
whether the derivation reaches it. No absent counter is reported as zero by assumption.
