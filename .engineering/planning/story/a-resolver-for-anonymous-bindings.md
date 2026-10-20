---
format: aep.planning-md/3
id: story:a-resolver-for-anonymous-bindings
kind: story
status: draft
title: A caller building a client for an anonymous binding uses a resolver llm exports
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 2
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
