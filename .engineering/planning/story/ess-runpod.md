---
format: aep.planning-md/3
id: story:ess-runpod
kind: story
status: implemented
title: The Runpod adapter is specified in ESS
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/runpod.rs
- confidence: cited
  path: contracts/runpod/scenarios
- confidence: cited
  path: spec/domains/runpod.yaml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-09-27T12:39:24Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-09-27T12:39:29Z", actor: "human:timo", revision: 4, imported: true}
- {from: "active", to: "implemented", at: "2026-09-27T14:23:04Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Context

Operator rule, 2026-09-26: every non-tooling beyond10x repository is driven by ESS. `crates/llm-runpod`
has no domain of its own in `spec/domains/` (read from `origin/plan/llm-foundation`, 2026-09-26):
the wave 2 adapter proves single-flight
startup, recovery, ownership and cleanup with 52 crate cases and no scenario. This story retrofits it: declarations read from the shipped code and citing it, anything
not readable marked `UNMAPPED:`.

## Acceptance

An `llm.runpod` domain declares the adapter's lifecycle over the `llm.hosting`
contract, and its scenarios pass against the adapter with the in-process emulator.

## Evidence

`crates/llm-runpod`; `docs/verification/runpod.md`; `story:runpod-hosting`.

## Verification

Authored `ess-scenario/1` documents under the domain's scenario directory run through
`checks/conformance` against the real crate, three runs with identical counts and zero failed,
error, unsupported or skipped; every guarded behaviour has a falsification record. The whole
repository `conformance -- check` stays green. No paid provider call in the default gate.
