---
format: aep.planning-md/3
id: story:retry-classes-harness-retried
kind: story
status: implemented
title: llm decides which provider failures before output are retriable where Harness retried them
summary: Responses server_error and rate_limit_exceeded and Messages overloaded_error and api_error are final in llm 0.7.0; Harness retried them
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
- informed_by: review-result:adversary-w09-harness-pass-1
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/suite.json
- confidence: inferred
  path: crates/llm-messages/src/client.rs
- confidence: cited
  path: crates/llm-messages/src/decode.rs
- confidence: inferred
  path: crates/llm-responses/src/client.rs
- confidence: cited
  path: crates/llm-responses/src/stream.rs
- confidence: inferred
  path: crates/llm-responses/tests/retry_class.rs
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: docs/messages.md
- confidence: inferred
  path: docs/responses.md
- confidence: inferred
  path: spec/domains/messages.yaml
- confidence: inferred
  path: spec/domains/responses.yaml
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T02:18:39Z", actor: "human:timo", revision: 6}
- {from: "proposed", to: "active", at: "2026-10-20T02:18:39Z", actor: "human:timo", revision: 7}
- {from: "active", to: "implemented", at: "2026-10-20T04:49:35Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

Found by the adversary of wave 2026-10-20-w09 (review-result:adversary-w09-harness-pass-1) while
re-citing `docs/harness-parity.md` at tag 0.7.0. Harness's own wire crates retried four failure
classes that llm 0.7.0 reports as final:

- Responses: Harness retried `server_error` and `rate_limit_exceeded`; llm marks only
  `server_is_overloaded` retriable, and only before output
  (`crates/llm-responses/src/stream.rs:477`).
- Messages: Harness retried `overloaded_error` and `api_error` (its deleted
  `harness-messages/tests/provider_emulated.rs:447` at Harness `2fd7235b` asserts "after 4
  attempts"); llm sets no retry mark on a Messages `stream_error`
  (`crates/llm-messages/src/decode.rs:750`).

After the Harness cutover (Atlas ADR 0097) the bridge passes `may_retry()` to Harness's loop
(`harness-cli/src/llm_port.rs:622` on the spike), so those retries stop. Rows R28 and M27 of
`docs/harness-parity.md` say `partial` for this reason since wave 2026-10-20-w09.

## Outcome

llm decides, per class, whether a failure before output is retriable, records the decision in
`docs/design.md`, and either marks the class retriable or states in R28 and M27 why it does not.

## Acceptance

- `docs/design.md` names each of the four classes and the decision for it, with the provider
  documentation the decision rests on.
- For each class marked retriable: a test per protocol that the class before output is retriable
  and after any output is final (the existing overload rule's shape,
  `crates/llm-responses/tests/retry_class.rs`).
- R28 and M27 in `docs/harness-parity.md` read `covered`, or `partial` with the reason.

## Design decisions

Taken 2026-10-20 by session `adr-changes` under the owner's grant
(`approval-record:wave-2026-10-20-w09-retry-classes`).

- Mark Responses `server_error` and `rate_limit_exceeded`, and Messages `overloaded_error` and
  `api_error`, retriable before any output (visible or silent, as the overload rule counts it) and
  final after output: the shape of `server_is_overloaded` today
  (`crates/llm-responses/src/stream.rs:477`).
- Cite in `docs/design.md` the provider documentation that lists each as transient (Anthropic's
  errors page for 529 and 500; OpenAI's error codes for 500 and 429). A class a provider page says
  not to retry stays final, and R28 or M27 says why (for example a 429 that means an exhausted
  quota, not a rate limit).
- An `unauthorized` failure still never falls back; fallback still tries only the declared
  compatible order.

## Scope

Written 2026-10-20 by the coordinator from the adversary's finding and a read of the tree;
**cited** = read, **inferred** = a reading.

- **Files:** `crates/llm-responses/src/stream.rs:477` (the retry mark) — cited
- **Files:** `crates/llm-messages/src/decode.rs:750` (`stream_error`, no retry mark) — cited
- **Tests:** `crates/llm-responses/tests/retry_class.rs`, a Messages counterpart under `crates/llm-messages/tests/` — inferred
- **Also likely:** where a non-streamed HTTP error status is classified for each client (`crates/llm-responses/src/client.rs`, `crates/llm-messages/src/client.rs`) — inferred
- **Spec and contracts:** `spec/domains/responses.yaml`, `spec/domains/messages.yaml`, scenarios under `contracts/{responses,messages}/scenarios/`, `contracts/ess-inputs.yaml`, regenerated `contracts/suite.json` — inferred
- **Documents:** `docs/design.md`, `docs/responses.md`, `docs/messages.md`, `docs/harness-parity.md` rows R28 and M27, the AGENTS.md invariant row for the overload rule — inferred
- **Confidence:** medium
- **Would collide with:** `story:text-deltas-are-final` on `stream.rs` and `decode.rs`; `story:tool-result-rejected-or-failed` on `docs/design.md` and the protocol pages
