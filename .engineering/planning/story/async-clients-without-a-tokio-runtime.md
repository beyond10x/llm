---
format: aep.planning-md/3
id: story:async-clients-without-a-tokio-runtime
kind: story
status: implemented
title: A caller without a Tokio runtime can run a turn, or is told by a typed error which adapter it needs
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: crates/llm-http/src/lib.rs
- confidence: cited
  path: crates/llm-http/src/transport.rs
- confidence: inferred
  path: crates/llm-http/tests
- confidence: cited
  path: docs/design.md
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T15:37:06Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T15:37:06Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-20T21:51:16Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100: gaps in the model layer go to llm). The
HTTP clients drive every exchange with Tokio: `HttpClient::post_sse_until` races the request against
cancellation with `tokio::select!` and bounds it with `tokio::time::timeout_at`
(`crates/llm-http/src/transport.rs:210-213`), and the request itself runs on `reqwest`, which needs a
Tokio reactor. A caller whose own executor is not Tokio has no asynchronous model to use. What such
a call does today (a panic for want of a reactor, or an error) is inferred, not observed.

A synchronous caller is served already: `b10x-llm-blocking`'s `BlockingModel::new` owns a
one-worker runtime and runs a turn on the calling thread (`crates/llm-blocking/src/lib.rs:3-11`).
The gap is an asynchronous caller on another executor, and that nothing outside the guides'
`Cargo.toml` lines says the async clients need Tokio
(`website/docs/guides/call-a-local-endpoint.md:46`).

## Outcome

An asynchronous caller without a Tokio runtime can run a turn, or learns from llm's documentation
and from a typed error, not a panic, that the clients need one and which adapter serves it instead.

## Acceptance

- A test calls a client's turn outside any Tokio runtime and asserts what happens; the observed
  behaviour is recorded in `docs/design.md`.
- Either the clients run a turn without an ambient Tokio runtime (for example by owning one, as
  `BlockingModel` does), with a test on a non-Tokio executor; or the call returns a typed error
  naming the requirement, and the crate documentation of `b10x-llm-http` and each protocol client
  states it and points at `b10x-llm-blocking`.

## Design decisions

Approved with wave 2026-10-20-w11 by session `b10x-chef-orchestrator` under the owner's grant:
option B. A client call outside a Tokio runtime returns a typed error that names the requirement
and `b10x-llm-blocking`, in place of what it does today (measured first and recorded in
`docs/design.md`); the asynchronous clients do not own a runtime. No new `ErrorCode` variant.
