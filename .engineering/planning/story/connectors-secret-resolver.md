---
format: aep.planning-md/1
id: story:connectors-secret-resolver
kind: story
status: draft
title: Connectors implements the injected secret resolver
relations:
- decomposes: epic:connectors-secret-adapter
- depends_on: story:secret-resolver
scope:
- confidence: inferred
  path: crates/llm-credentials
- confidence: inferred
  path: spec/domains/catalog.yaml
revision: 2
---
## Context

Use a released arbitrary-secret contract independent of upstream connector ownership. Do not assume current Connectors credential custody permits arbitrary reads.

## Acceptance

The same route TOML works with caller-injected and Connectors-backed resolution, including missing/revoked/rotated secrets, without a core dependency or secret disclosure.

## Verification

Contract fixtures and an exact released Connectors SDK revision; preserve source refs and redaction errors.

## Scope

- inferred: crates/llm-credentials
- inferred: spec/domains/catalog.yaml
