---
format: aep.planning-md/3
id: story:serving-extraction
kind: story
status: implemented
title: The gateway, hosting and provisioning crates live in llm-gateway, not llm
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: .github/workflows
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: README.md
- confidence: inferred
  path: Taskfile.yml
- confidence: inferred
  path: checks/conformance
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-gateway
- confidence: inferred
  path: crates/llm-modal
- confidence: inferred
  path: crates/llm-provision
- confidence: inferred
  path: crates/llm-runpod
- confidence: inferred
  path: docs
- confidence: inferred
  path: spec
revision: 20
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T09:59:35Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-05T09:59:35Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-05T10:17:49Z", actor: "human:timo", revision: 20, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Outcome

The crates `llm-gateway`, `llm-provision`, `llm-runpod` and `llm-modal`, with their history, tests,
ESS sources and planning artifacts, live in the new repository `beyond10x/llm-gateway`;
`beyond10x/llm` no longer contains them and releases without them. `beyond10x/llmgw` is not
changed by this story.

## Acceptance

- `beyond10x/llm` `main`: `cargo metadata --no-deps` lists no package whose name contains
  `gateway`, `provision`, `runpod` or `modal`; `task check` exits 0.
- The `llm-gateway` repository: `task check` exits 0 on `main`; its four crates depend on llm's
  client crates only by tag, never by path.
- Each moved story (`story:gateway-auth`, `story:gateway-translation`, `story:hosting-contract`,
  `story:runpod-hosting`, `story:ess-gateway`, `story:ess-runpod`, and `epic:hosting`
  with the gateway half of `epic:gateway`) exists in the new store with its evidence, and is `superseded` here by it.
- Gates enrollment and Atlas catalog entries exist for the new repository; llm's catalog entry
  names the client side only.
- An llm release without the serving crates is cut through llm's own release process.

## Decision

New repository, operator 2026-10-05 ("gateway -> B"), recorded on
`decision-blocker:gateway-repository-home`. `beyond10x/llmgw` (2,857 src lines, llmgw `048ebd8`)
stays in service until `story:llmgw-retirement`.

## Scope (inferred)

llm: `crates/llm-gateway`, `crates/llm-provision`, `crates/llm-runpod`, `crates/llm-modal`,
`Cargo.toml` workspace members, `ess/` gateway and runpod domains, `.engineering/`.
New or renamed repository: the same crates. gates-policy: enrollment. Atlas: catalog entry.
