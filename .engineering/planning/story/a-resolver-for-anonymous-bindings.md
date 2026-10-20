---
format: aep.planning-md/3
id: story:a-resolver-for-anonymous-bindings
kind: story
status: active
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
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T15:37:04Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T15:37:05Z", actor: "human:timo", revision: 6}
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
export the resolver `b10x-llm-models` keeps private (`Anonymous`, `crates/llm-models/src/lib.rs:49-60`
at 0.8.2) from `b10x-llm-credentials`, refusing every resolve with a typed error as it does today;
`b10x-llm-models` uses the exported one and drops its copy.
