---
format: aep.planning-md/3
id: story:catalog-model-port
kind: story
status: active
title: One exported function builds the Model a catalog serving model declares
summary: Protocol picks ChatClient, ResponsesClient or MessagesClient; the account picks the caller's credential resolver; callers stop writing that match
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: checks/conformance
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-core/tests/dependency_boundary.rs
- confidence: cited
  path: crates/llm-docs/examples/local_endpoint.rs
- confidence: inferred
  path: crates/llm-models
- confidence: cited
  path: crates/llm-routing/src/fallback.rs
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: spec/domains/routing.yaml
- confidence: inferred
  path: website/docs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-08T07:35:39Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-08T07:35:39Z", actor: "human:timo", revision: 6}
---
## Context

Every caller that turns an `llm.catalog/1` serving model into a usable `llm_core::Model` writes the same match today: the binding's `Protocol` picks `ChatClient`, `ResponsesClient` or `MessagesClient`, and the caller picks the credential resolver for the binding's account by hand (`crates/llm-docs/examples/local_endpoint.rs:25-34`; `llm_routing::Models` at `crates/llm-routing/src/fallback.rs:21-24` leaves the whole construction to the caller). Two consumers, loom's catalog route and llm-gateway's Loom qualification, need this construction and should not each copy it.

## Design

- A new workspace crate `crates/llm-models` (package `b10x-llm-models`) owns the construction. It depends on `llm-core`, `llm-http`, `llm-credentials`, `llm-providers`, `llm-routing`, `llm-chat`, `llm-responses` and `llm-messages`. `llm-routing` keeps no dependency on any protocol crate.
- One exported function builds one port from one validated binding: the binding's protocol selects the client; the account selects the resolver through a caller-supplied lookup keyed by account id. An anonymous account needs no resolver. A credentialed account without one is refused before any I/O as `unauthorized`, never silently given a resolver that answers `Missing`.
- One exported type builds a port for every serving model of a `Catalog` with the same rules and implements `llm_routing::Models`, so ordered fallback consumes it directly.
- The construction performs no network I/O and resolves no secret; resolution stays at turn time.

## Spec first

Model the construction in the ESS specification (`spec/domains/routing.yaml` or a new domain), as a command whose observed outcome names the protocol port built for a serving model, or the typed refusal. Validate with the newest `ess`, regenerate `contracts/suite.json` and `contracts/schema/`, add the conformance adapter, and list the new scenarios in `contracts/ess-inputs.yaml`.

## Acceptance

Conformance scenarios, all passing in `task conformance` with no skips and the baseline raised by their count:
- a Chat Completions serving model builds the Chat port;
- a Responses serving model builds the Responses port;
- a Messages serving model builds the Messages port;
- an anonymous account builds without any resolver;
- a credentialed account without a resolver is refused `unauthorized` before any I/O;
- an unknown serving model id is refused;
- the catalog-wide builder answers `Models::model` for every declared serving model and `None` for an undeclared id.

`crates/llm-docs/examples/local_endpoint.rs` uses the function in place of its own match, and the guide page quoting it is regenerated.

## Verification

`task conformance`; `cargo test -p b10x-llm-models`; `task docs`; the dependency-boundary test still passes and covers the new crate. No paid provider call.
