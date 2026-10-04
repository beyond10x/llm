---
format: aep.planning-md/3
id: story:ess-http-transport
kind: story
status: implemented
title: The HTTP transport is specified in ESS
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/transport.rs
- confidence: cited
  path: contracts/transport/scenarios
- confidence: cited
  path: spec/domains/transport.yaml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-09-27T12:39:02Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-09-27T12:39:08Z", actor: "human:timo", revision: 4, imported: true}
- {from: "active", to: "implemented", at: "2026-09-27T14:23:40Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Context

Operator rule, 2026-09-26: every non-tooling beyond10x repository is driven by ESS. `crates/llm-http`
has no domain of its own in `spec/domains/` (read from `origin/plan/llm-foundation`, 2026-09-26):
bounded streaming, termination, cancellation and
the disabled ambient proxy (`f27b836`) are proved by crate tests only. This story retrofits it: declarations read from the shipped code and citing it, anything
not readable marked `UNMAPPED:`.

## Acceptance

An `llm.transport` domain declares the bounds, termination and cancellation the crate
ships, and its scenarios pass against the real transport over a local server.

## Evidence

`crates/llm-http`; `story:http-streaming`.

## Verification

Authored `ess-scenario/1` documents under the domain's scenario directory run through
`checks/conformance` against the real crate, three runs with identical counts and zero failed,
error, unsupported or skipped; every guarded behaviour has a falsification record. The whole
repository `conformance -- check` stays green. No paid provider call in the default gate.
