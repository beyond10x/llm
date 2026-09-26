---
format: aep.planning-md/1
id: story:secrets-resolver
kind: story
status: draft
title: Inference resolves credentials through the secrets library
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
- depends_on: story:secret-resolver
- supersedes: story:connectors-secret-resolver
scope:
- confidence: cited
  path: contracts/secrets/scenarios
- confidence: cited
  path: crates/llm-credentials/Cargo.toml
- confidence: cited
  path: crates/llm-credentials/src/secrets.rs
- confidence: cited
  path: spec/domains/secrets.yaml
revision: 2
---
## Context

`llm-credentials` resolves secrets through the injected `SecretResolver`
(`crates/llm-credentials/src/lib.rs:134-148`). The `secrets` repository now specifies a named,
scoped, federated store (`secrets.storage`, secrets `spec/domains/storage.yaml`). This story
adds an optional `secrets` feature with a `SecretsResolver` over `SecretStorage`: a route's
`SecretRef` is a secrets `SecretName`, the scope is configured once, `resolve` reads, and
`refresh` re-reads and is `RefreshRejected` when the version did not change. It supersedes
`story:connectors-secret-resolver`: Connectors is out of scope this round (operator,
2026-09-26), and custody moves to the `secrets` library instead.

## Acceptance

The same route TOML resolves a credential through the `secrets` library with the
keychain backend, and missing, denied, rotated and unsupported cases map to the existing
`SecretError` codes with no secret bytes in any diagnostic; core crates gain no dependency on
`secrets`.

## Evidence

secrets `spec/domains/storage.yaml`; `crates/llm-credentials`; the `llm.secrets`
domain grows to observe the new adapter. Depends on the first `secrets` release that ships
`story:storage-port` and `story:keychain-backend` (secrets store); pin that exact revision.

## Verification

Authored `ess-scenario/1` documents under the domain's scenario directory run through
`checks/conformance` against the real crate, three runs with identical counts and zero failed,
error, unsupported or skipped; every guarded behaviour has a falsification record. The whole
repository `conformance -- check` stays green. No paid provider call in the default gate.
