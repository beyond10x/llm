---
format: aep.planning-md/1
id: story:local-secret-adapters
kind: story
status: draft
title: Optional local secret adapters resolve explicit references
relations:
- decomposes: epic:access
- depends_on: story:secret-resolver
scope:
- confidence: cited
  path: .github/workflows/gate.yml
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: Taskfile.yml
- confidence: cited
  path: checks/conformance
- confidence: cited
  path: contracts
- confidence: cited
  path: contracts/secrets
- confidence: inferred
  path: crates/llm-credentials
- confidence: cited
  path: docs/local-secrets.md
- confidence: cited
  path: spec
revision: 8
---
## Context

Backend feature selection must not affect remote embeddings. Use protected file checks and mocked keychain tests; document runtime injection for remote services. Secret entry never goes through argv, TOML, tracing or diagnostic output.

## Acceptance

Keychain and explicit mounted-file adapters resolve the same SecretRef contract and refuse absent or unsafe sources without any ambient credential search.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Implemented opt-in `file`, `keychain` and `native-keychain` features. Linux protected-file reads
and explicit keyring-core store injection share SecretRef, safe errors and redacted content
versions. Native constructors compile for Linux Secret Service, macOS Keychain and Windows
Credential Manager; CI separately tests compilation on the latter two. No test reads user secrets.
File resolution refuses non-Linux platforms because ACL protections there are not implemented.

`task check` passed 56 runtime tests and two compile-fail documentation tests, default and
all-feature credentials builds, strict Clippy/formatting, ESS schema/suite drift and AEP validation.
The combined complete suite has 52 passed, zero failed/error/unsupported/skipped in three
consecutive runs; 49 authored scenarios and three generated probes, zero synthesis refusals.
This retains the preceding 28 routing scenarios and adds 22 authored secret behaviors plus two
generated probes. The target reads real temporary files and explicitly injected mock keychains.

Nine restored production mutations each failed a named authored scenario. Wrong-entry lookup also
caused one generated-fixture error from a pending injected fault; it is retained and separately
reported, not counted as a behavioral assertion. Restored-source runs have zero target errors.
See docs/verification/local-secrets.md, local-secrets-report.json and local-secrets-falsification.json
in that directory, paired with contracts/suite.json. Native OS unlock/service availability,
foreign-owner files and adversarial metadata races remain outside the measured fixture evidence.

The planned full foundation and release remain incomplete. This ordinary-session implementation
does not claim a governed drive ran; Chat's map and USD inputs remain pending. No lifecycle moves
were made. Exact remote CI is checked after publication; local success does not assert it.

## Scope

- cited: `crates/llm-credentials` — optional file/keychain implementations and runtime tests.
- cited: `spec`, `contracts`, `checks/conformance` — declarations, real-library observations, exact
  scenario selection, generated projections and report gate; coordinated with runtime-contracts.
- cited: `Cargo.lock`, `Taskfile.yml`, `.github/workflows/gate.yml` — optional native dependencies and gates.
- cited: `docs/local-secrets.md`, `docs/verification/local-secrets.md` — usage, evidence and explicit limits.

## Implementation contract

The governed chat-projection run remains pending operator map selection and USD terms. This
independent required story proceeds as ordinary-session implementation; it is not relabeled as
a driven run, and no artifact lifecycle moves are made beside the driver.

Implement opt-in `file` and `keychain` features in llm-credentials. Both resolve only an explicit
map of SecretRef to a source; neither searches vendor directories, mutates credentials, falls back
to another backend, trims bytes or resolves configuration during construction. Remote applications
continue injecting SecretResolver without compiling native backends. Keychain uses an explicitly
injected keyring-core CredentialStore; optional native feature constructors select an actual OS
backend only when the caller asks. No global default store is changed.

File bindings are absolute lexical paths. Linux reads walk directory descriptors without following
symlinks, validate trusted directory ownership/write modes, and admit only a single-link regular
file owned by the effective user or root with owner-only read/write permissions. Reads are bounded
and metadata changes during a read refuse. Unsupported platforms refuse instead of weakening checks.
Read-only generation identity follows exact credential bytes, so same bytes retain the same version;
restoring old bytes restores that identity. These adapters never implement credential renewal.

All blocking work has bounded concurrency, with permits held by the blocking operation even if the
async caller cancels. Backends may not support interruption; dropping a future does not imply an
in-flight OS read stopped. Backend errors are converted to fixed safe SecretError variants and
material is zeroized/redacted, never serialized into route config or diagnostics.

ESS declarations for file/keychain bindings precede code. Extend behavioral conformance to actual
protected-file and mocked-keychain observations, retain report counts and mutation evidence, and
run default plus feature gates. Runtime mocks establish adapter behavior, not live OS-service
qualification. No existing user keychain entry or credential file is read or modified by tests.
