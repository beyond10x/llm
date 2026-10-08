---
format: aep.planning-md/3
id: story:credentials-from-secrets
kind: story
status: draft
title: Credentials come only from the secrets library; llm reads no token file
relations:
- decomposes: epic:access
- depends_on: story:secrets-resolver
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: checks/conformance/Cargo.toml
- confidence: inferred
  path: checks/conformance/src/secrets.rs
- confidence: inferred
  path: contracts/baseline.json
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/schema
- confidence: inferred
  path: contracts/secrets/scenarios
- confidence: inferred
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-credentials
- confidence: cited
  path: crates/llm-credentials/Cargo.toml
- confidence: cited
  path: crates/llm-credentials/src/codex
- confidence: cited
  path: crates/llm-credentials/src/codex.rs
- confidence: cited
  path: crates/llm-credentials/src/file.rs
- confidence: inferred
  path: crates/llm-credentials/src/lib.rs
- confidence: cited
  path: crates/llm-credentials/src/pointer.rs
- confidence: inferred
  path: crates/llm-credentials/tests
- confidence: inferred
  path: crates/llm-routing/Cargo.toml
- confidence: inferred
  path: crates/llm-routing/tests/codex_login_fallback.rs
- confidence: inferred
  path: crates/llm-routing/tests/credential_fallback.rs
- confidence: inferred
  path: crates/llm-tool-call/Cargo.toml
- confidence: inferred
  path: crates/llm-tool-call/src/lib.rs
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: docs/local-secrets.md
- confidence: inferred
  path: spec/domains/catalog.yaml
- confidence: inferred
  path: spec/domains/secrets.yaml
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/concepts/credentials.md
- confidence: inferred
  path: website/docs/guides/resolve-a-local-secret.md
revision: 4
---
## Outcome

llm reads no credential from a file of its own choosing: every `SecretRef` resolves through the
secrets library (`story:secrets-resolver`). The file-reading adapters in `llm-credentials`
(`FileResolver`, `CodexAuthFile` and the JSON-pointer resolver over a file) are retired or moved
behind the secrets library.

## Why

Operator, 2026-10-05: "we dont want to have a file with tokens at all, stuff must come dfrom
b10x/secrets" (approval-record:access-decisions-2026-10-05).

## Open

- UNMAPPED: the Codex login lives in Codex's own `auth.json` and is renewed in place
  (`story:parity-codex-renewal`). Whether the secrets library takes custody of it, reads it, or
  the Codex route moves elsewhere is not decided; ask the operator before this story is scoped.
- UNMAPPED: Harness and Loom consumers that configure a file source today.

## Acceptance

`cargo tree -e features` shows no file-reading credential feature enabled by any consumer route;
the secrets-library route resolves every credential the qualification uses.

## Scope

Scoped 2026-10-08 at 560f044c; confidence medium. The Codex line items stay inferred until the first Open question (what happens to the Codex `auth.json`) is answered.

- Primary surface: `crates/llm-credentials` (`FileResolver` `src/file.rs:13`, `CodexAuthFile` `src/codex.rs:42` and `src/codex/`, `JsonPointerResolver` `src/pointer.rs:20`, features `file`, `codex-auth-file`, `codex-renewal`, `json-pointer`).
- Consumers: `crates/llm-tool-call` (the one non-dev consumer of `codex-auth-file`), `crates/llm-routing` dev tests `codex_login_fallback.rs` and `credential_fallback.rs`, which hold the "unauthorized never falls back" invariant and must be re-homed, not deleted.
- Conformance: `checks/conformance/src/secrets.rs`, `contracts/secrets/scenarios` (`file-*`, `codex-*`), `contracts/baseline.json` (a lower count needs a CHANGELOG reason), `spec/domains/catalog.yaml` (`MountedSecretFile`, `CodexAuthFile`), `spec/domains/secrets.yaml`.
- Documents: `docs/local-secrets.md`, `docs/harness-parity.md`, `docs/implementation-status.md`, the credentials concept page and the local-secret guide.
- Collides with anything touching `llm-credentials` features, the `llm.secrets` or `llm.catalog` domains, or `contracts/baseline.json`.
