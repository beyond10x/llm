---
format: aep.planning-md/1
id: story:ess-gateway
kind: story
status: draft
title: The gateway surface is specified in ESS
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/gateway.rs
- confidence: cited
  path: contracts/gateway/scenarios
- confidence: cited
  path: spec/domains/gateway.yaml
revision: 2
---
## Context

Operator rule, 2026-09-26: every non-tooling beyond10x repository is driven by ESS. `crates/llm-gateway`
has no domain of its own in `spec/domains/` (read from `origin/plan/llm-foundation`, 2026-09-26):
its authenticated single-owner surface and
read-only route inspection, implemented in wave 1 (`story:gateway-auth`), are proved by crate
tests only. This story retrofits it: declarations read from the shipped code and citing it, anything
not readable marked `UNMAPPED:`.

## Acceptance

An `llm.gateway` domain declares the owner authentication and route inspection the
crate ships, and its scenarios pass against the real gateway over a loopback socket.

## Evidence

`crates/llm-gateway`; `docs/gateway.md`; `story:gateway-auth`.

## Verification

Authored `ess-scenario/1` documents under the domain's scenario directory run through
`checks/conformance` against the real crate, three runs with identical counts and zero failed,
error, unsupported or skipped; every guarded behaviour has a falsification record. The whole
repository `conformance -- check` stays green. No paid provider call in the default gate.
