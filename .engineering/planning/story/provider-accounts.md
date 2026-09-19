---
format: aep.planning-md/1
id: story:provider-accounts
kind: story
status: active
title: Provider accounts are independent of wire selection
relations:
- decomposes: epic:access
- depends_on: story:secret-resolver
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-providers
revision: 7
---
## Context

Define provider facts, account/billing identity, endpoint, auth kind and protocol as distinct values. Include caller-managed API/subscription sources and anonymous explicitly selected local endpoints; no built-in-name restriction. Validate configuration without resolving secrets.

## Acceptance

A caller binds arbitrary provider/account IDs to endpoints while independently selecting a supported protocol and model. An explicitly anonymous binding succeeds without a credential reference or resolver call; every authenticated binding requires a reference and refuses a missing reference during configuration validation. No automatic downgrade to anonymous access is permitted.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `crates/llm-providers` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.

## Binding contract

Existing typed homes are spec/domains/catalog.yaml Provider, Account, Endpoint, Model and
ServingModel, with their declared references. Account now explicitly carries api_key_header
exactly when auth_kind is ApiKey; Bearer and Anonymous cannot carry it. Authentication never
follows from protocol. Capabilities is the sole home for serving context/output limits, removing
the redundant draft ServingModel.context_window field before first release.

Implement a strict llm.binding/1 document and an immutable validated binding. Validation checks
all references, capabilities, secret-reference/auth invariants and bounded HTTP base URLs without
I/O. An endpoint is an explicit API base prefix; standard protocol paths append to it. Reject
userinfo/query/fragment credentials and unsupported schemes. Arbitrary operator IDs and upstream
model names are supported without vendor-name matching. This is a binding document; the multi-route
TOML catalog remains catalog-routing's responsibility.

Request-time auth presentation resolves only the named reference, with cancellation and no
fallback or automatic refresh/resend. API-key header names are explicit and cannot override HTTP
routing/framing headers. Prepared auth preserves the caller's generation for later qualified
refresh decisions. Successful local auth preparation is not a live provider qualification.

## Implementation evidence

Implemented independent provider, account, endpoint, served-model and serving-binding declarations,
strict llm.binding/1 validation, arbitrary HTTP(S) base URLs, protocol-specific operation paths,
request-time selected-reference authentication, redacted sensitive headers and explicit anonymous
access. Billing and authentication remain separate from protocol; the 27-combination matrix is
covered by Rust fixtures. Catalog inspection never resolves credentials.

Full task check passes on 2026-09-19: 50 runtime tests plus 2 compile-fail docs. The real provider
bindings are also exercised through 28 passing ESS routing scenarios; zero skips or refusals.
Missing-reference acceptance and wrong Messages path mutations each fail a named scenario.
See executable-system-specification:routing-observations and docs/verification/routing-conformance.md.
API/subscription access qualification and protocol client execution remain separate work. Keep the
story active while the foundation and release requirements remain incomplete.
