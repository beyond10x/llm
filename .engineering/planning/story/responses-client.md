---
format: aep.planning-md/3
id: story:responses-client
kind: story
status: draft
title: A Responses client sends one neutral turn and decodes its stream
relations:
- decomposes: epic:access
- depends_on: story:responses-projection
scope:
- confidence: cited
  path: crates/llm-responses/Cargo.toml
- confidence: cited
  path: crates/llm-responses/src/client.rs
- confidence: cited
  path: crates/llm-responses/src/lib.rs
- confidence: cited
  path: crates/llm-responses/tests/client.rs
revision: 2
---
## Outcome

`llm-responses` gains a client, as `llm-messages` has one: `ResponsesClient::new(binding,
HttpClient, Arc<dyn SecretResolver>)` implements `llm_core::Model`.

- It projects a neutral `TurnRequest` with the existing `project_request`.
- It sends one streaming `POST {base_url}responses` through `llm-http`, with the bearer the resolver
  returns.
- It decodes the stream with the existing `decode_stream` into a `TurnOutcome`.
- `ToolChoice::Named` reaches the wire as a forced function call.
- It makes one attempt, with no retry or fallback (the same contract as `MessagesClient`).
- Request headers are fixed by the binding: no originator or session header the caller did not
  declare.

## Acceptance

`a_responses_turn_returns_its_function_call`, against a local HTTP fixture that streams a Responses
answer with one `function_call`:
- the outcome carries that call's arguments, and the request body names the forced tool;
- a stream that ends before `response.completed` gives the transport error;
- a resolver error gives the credential error, before any request is sent.

## ESS first

None: no `spec/` noun changes; the Responses protocol is declared already (`responses-projection`).
The first commit is the named test, red because the client does not exist.

## Reuses

`crates/llm-messages/src/client.rs` (shape), `crates/llm-responses/src/{request,stream,binding}.rs`,
`crates/llm-http/src/transport.rs`.
