---
format: aep.planning-md/3
id: story:a-resolver-for-anonymous-bindings
kind: story
status: implemented
title: A caller building a client for an anonymous binding uses a resolver llm exports
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: crates/llm-chat/tests
- confidence: cited
  path: crates/llm-credentials/src/lib.rs
- confidence: inferred
  path: crates/llm-credentials/tests
- confidence: cited
  path: crates/llm-models/src/lib.rs
- confidence: cited
  path: website/docs/guides/call-a-local-endpoint.md
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T15:37:04Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T15:37:05Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-20T21:51:16Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100). `ChatClient::new(binding, http, resolver)`
requires an `Arc<dyn SecretResolver>` (`crates/llm-chat/src/client.rs:31`), and so do the Responses
and Messages clients. For a keyless local server (an anonymous account) llm has a resolver that
refuses every call, `Anonymous`, but it is private to `b10x-llm-models`
(`crates/llm-models/src/lib.rs:49-60`) and only the catalog path hands it out
(`crates/llm-models/src/lib.rs:105`, `:120`). A caller that builds a protocol client directly
writes its own; inference copies about ten lines from one of llm's tests.

## Outcome

A caller building a protocol client for an anonymous binding uses a resolver llm exports.

## Acceptance

- `b10x-llm-credentials` exports a resolver for anonymous bindings that refuses every resolve with
  a typed error (the behaviour `Anonymous` has), and `b10x-llm-models` uses it in place of its
  private copy.
- A test builds `ChatClient` with it against a local fixture server and runs a turn with no
  `Authorization` header sent; a test asserts it refuses a resolve.
- The local-endpoint guide names it for a caller that does not go through the catalog.

## Design decisions

Approved with wave 2026-10-20-w11 by session `b10x-chef-orchestrator` under the owner's grant:
export the resolver `b10x-llm-models` kept private (`Anonymous`, `crates/llm-models/src/lib.rs:49-60`
at 0.8.2) from `b10x-llm-credentials` as `AnonymousResolver`; `b10x-llm-models` uses it and drops
its copy.

Changed by the coordinator after adversary pass 1 (`review-result:adversary-w11-resolver-pass-1`):
it refuses every resolve with `SecretError::Missing`, not `InvalidReference` as the private copy did.
Exported, it can be handed to a credentialed binding by mistake; `Missing` maps to `unauthorized`,
which stops the run before sending and takes no fallback (`crates/llm-providers/src/auth.rs:109-121`),
where `InvalidReference` read as `unavailable` and fell back to the next target.
