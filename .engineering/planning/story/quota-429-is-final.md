---
format: aep.planning-md/3
id: story:quota-429-is-final
kind: story
status: active
title: An HTTP 429 a provider documents as an exhausted quota is final
summary: The transport retries every 429 and reads no body, so an insufficient_quota 429 is retried
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: crates/llm-http/src/transport.rs
- confidence: cited
  path: crates/llm-http/tests/retry_classes.rs
- confidence: inferred
  path: crates/llm-messages/src/client.rs
- confidence: cited
  path: crates/llm-messages/tests/retry_class.rs
- confidence: inferred
  path: crates/llm-responses/src/client.rs
- confidence: inferred
  path: crates/llm-responses/src/stream.rs
- confidence: cited
  path: crates/llm-responses/tests/adversary_w09_retry_http_quota.rs
- confidence: cited
  path: crates/llm-responses/tests/retry_class.rs
- confidence: inferred
  path: docs/contract-v1.md
- confidence: cited
  path: docs/design.md
- confidence: cited
  path: docs/harness-parity.md
- confidence: inferred
  path: docs/messages.md
- confidence: cited
  path: docs/responses.md
- confidence: inferred
  path: spec/domains/transport.yaml
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T07:47:07Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T07:47:07Z", actor: "human:timo", revision: 6}
---
## Context

Recorded by the implementor of `story:retry-classes-harness-retried` in wave 2026-10-20-w09: the
HTTP transport marks every 429 and every 500-599 retriable (`crates/llm-http/src/transport.rs`,
around `:336-340`) and reads no error body, so an OpenAI 429 whose body says `insufficient_quota`
(an exhausted quota, which the provider documents as not resolved by retrying) is retried.
`docs/design.md` records this limit since that wave.

## Outcome

An HTTP 429 that a provider documents as an exhausted quota is final.

## Acceptance

- The transport or the protocol client reads a bounded error body for 429 and classifies
  `insufficient_quota` as final, without copying provider prose into a diagnostic.
- A test per protocol pins a quota 429 as final and a rate-limit 429 as retriable.

## Scope

Derived 2026-10-20 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-http/src/transport.rs`: `status_error` `:336-350`, its 429 arm `:339`; `post_sse_until` `:224-230` is the one place that still holds the refused `Response`; the bounded-read pattern is `post_json_once` `:439-470` with `MAX_EXCHANGE_BYTES` `:479` — cited
- **Files:** `crates/llm-responses/src/client.rs:192-203`, `crates/llm-messages/src/client.rs:135-145`, only if classification sits in the client or `post_sse_until` takes a classifier — inferred
- **Symbols:** `status_error`, `HttpClient::post_sse_until`, `ResponseTap` — cited
- **Tests (edited):** `crates/llm-responses/tests/adversary_w09_retry_http_quota.rs:156` flips to final; `crates/llm-responses/tests/retry_class.rs:344-383` keeps the rate-limit 429 retriable beside a new quota case; `crates/llm-messages/tests/retry_class.rs:192-220`; `crates/llm-http/tests/retry_classes.rs:17-44` — cited
- **Tests (guards, stay green):** `crates/llm-http/tests/transport.rs:211` (no 429 body in the error), `:518` (the tap sees the head only), `crates/llm-responses/tests/adversary_client.rs:624` (a stalled 100 MB 429 body returns in under 2 s), `crates/llm-messages/tests/adversary_subscription.rs:171`, the AGENTS.md:53 tests `crates/llm-http/tests/transport.rs:98` and `crates/llm-http/tests/ambient_proxy.rs:14` — cited
- **Also likely:** `crates/llm-responses/src/stream.rs:741-776` if the billing codes are shared with the in-stream classifier — inferred
- **Documents:** `docs/design.md:205-213`, `AGENTS.md:50`, `docs/responses.md:414-416`, `docs/harness-parity.md:84`, `:321-325` — cited; `docs/contract-v1.md:86-90`, `spec/domains/transport.yaml`, `docs/messages.md:221-226` — inferred
- **Confidence:** high
- **Would collide with:** any unit touching `status_error`, `post_sse_until` or `ResponseTap`; the send path of `ResponsesClient::exchange` or `MessagesClient::stream`; the `retry_class.rs` tests; `docs/design.md` "Provider failures retried before output"; the AGENTS.md retry row — cited
- **Safety fact:** reading a 429 body sends no second request and changes no client option (`Policy::none()`, `retry::never()`, `no_proxy()` at `transport.rs:98-103`) — unproven

## Open for the design

- What marks a Messages quota 429 is not established: Anthropic's spend-cap 429 is `rate_limit_error`
  without a Retry-After header (`docs/design.md:200-202`); a header rule would also make the
  wrong-preamble 429 final (`crates/llm-messages/tests/subscription.rs:308`).
- The code chosen for "final" decides fallback: a non-retriable `RateLimited` still falls back to the
  next target (`crates/llm-routing/src/fallback.rs:230-235`, `:527-535`); a `Refused` ends the run,
  as the in-stream quota does (`crates/llm-responses/src/stream.rs:765-770`).
- How the classification reaches the client: `Error` is serialized with `deny_unknown_fields`
  (`crates/llm-core/src/error.rs:38`), so a new field is a format change; a parameter on
  `post_sse_until` reaches more callers.
