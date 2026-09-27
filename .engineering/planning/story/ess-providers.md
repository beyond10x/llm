---
format: aep.planning-md/2
id: story:ess-providers
kind: story
status: draft
title: Provider accounts and bindings are specified in ESS
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/providers.rs
- confidence: cited
  path: contracts/providers/scenarios
- confidence: cited
  path: spec/domains/providers.yaml
revision: 2
---
## Context

Operator rule, 2026-09-26: every non-tooling beyond10x repository is driven by ESS. `crates/llm-providers`
has no domain of its own in `spec/domains/` (read from `origin/plan/llm-foundation`, 2026-09-26):
provider accounts, bindings and
`prepare_auth` are observed only through the catalog and routing domains. This story retrofits it: declarations read from the shipped code and citing it, anything
not readable marked `UNMAPPED:`.

## Acceptance

An `llm.providers` domain declares account and binding preparation, including the
anonymous account and each secret-error mapping (`auth.rs:102-110`), and its scenarios pass
against the real crate with an injected resolver.

## Evidence

`crates/llm-providers/src/auth.rs:33-60,102-110`; `story:provider-accounts`.

## Verification

Authored `ess-scenario/1` documents under the domain's scenario directory run through
`checks/conformance` against the real crate, three runs with identical counts and zero failed,
error, unsupported or skipped; every guarded behaviour has a falsification record. The whole
repository `conformance -- check` stays green. No paid provider call in the default gate.
