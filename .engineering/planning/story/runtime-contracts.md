---
format: aep.planning-md/1
id: story:runtime-contracts
kind: story
status: active
title: Publish the neutral contract and compatibility policy
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: .github/workflows
- confidence: cited
  path: AGENTS.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: cited
  path: README.md
- confidence: inferred
  path: Taskfile.yml
- confidence: cited
  path: checks/conformance
- confidence: cited
  path: contracts
- confidence: cited
  path: docs/contract-v1.md
- confidence: inferred
  path: docs/design.md
- confidence: cited
  path: docs/implementation-status.md
- confidence: cited
  path: docs/verification/core-foundation.md
- confidence: cited
  path: docs/verification/routing-conformance.md
- confidence: cited
  path: docs/verification/routing-falsification.json
- confidence: cited
  path: docs/verification/routing-report.json
- confidence: cited
  path: spec
revision: 12
---
## Context

Refine the ESS declaration model into typed protocol/auth/capability vocabularies before runtime code. Define async streaming/cancellation and ownership, opaque provenance, optional usage, protocol-neutral tools without authority, serialization stability and supported text/tool scope. Preserve current source provenance. Pin AEP/ESS/toolchain and gate/release workflows; no provider calls in ordinary CI.

## Acceptance

A versioned contract declares the supported turn, configuration, error and compatibility surfaces and all dependency-boundary checks pass.

## Evidence

Operator-approved design, 2026-09-19; docs/design.md; spec/system.yaml and spec/domains/catalog.yaml. Existing source references are listed under docs/design.md, Source evidence and draft limits.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

- inferred: `spec` — planned implementation surface.
- inferred: `docs/design.md` — planned implementation surface.
- inferred: `Cargo.toml` — planned implementation surface.
- inferred: `Cargo.lock` — planned implementation surface.
- inferred: `.github/workflows` — planned implementation surface.
- inferred: `Taskfile.yml` — planned implementation surface.

Shared specification and workspace manifests are integration surfaces: coordinate changes through their owning story; do not infer parallel safety from different crate names.

## Implementation progress

Implemented `docs/contract-v1.md`, typed protocol/auth/billing/capability vocabularies in
`spec/domains/catalog.yaml`, and dependency-boundary checks. The turn and outcome envelopes
reject unknown versions and fields. `task check` passed on 2026-09-19; exact fixture identities
and limits are in `docs/verification/core-foundation.md`.

Remaining: versioned provider/routing configuration and source release/common Gates setup.
Keep this story active; a compiling workspace or local library tests do not qualify a release.
Current complete-milestone tracking is `docs/implementation-status.md`.

Additional cited scope: `README.md`, `AGENTS.md`, `docs/contract-v1.md`,
`docs/implementation-status.md`, and `docs/verification/core-foundation.md` describe the
implemented contract and its verification without claiming planned capabilities.

## Binding revision correction

Configuration integration exposed a gap in opaque provenance: IDs alone cannot detect an endpoint
URL, upstream model, auth reference or capability declaration changed while keeping its operator
ID. Provenance now requires binding_revision, typed in spec/domains/catalog.yaml. Provider bindings
derive it from SHA-256 of the complete validated single-binding declaration; secret bytes and
credential generations are excluded so ordinary rotation does not invalidate continuation state.

This changes the serialized neutral contract: llm.turn/2 and llm.outcome/2 replace the unreleased
v1 envelopes. Old and future versions are explicitly refused, never silently assigned a revision.
The Rust workspace remains 0.0.0 with no consumer adoption or release. Add same-ID repointing
regressions before protocol clients build on the contract.

## Governed driver handoff

The operator explicitly requested worktree, aep-plan:planning and aep-drive:drive on 2026-09-19.
No further lifecycle moves were made after invoking drive. The existing implementation remains
ordinary-session work; it is not relabeled as a governed result.

Draft task: .engineering/task-chat-projection.yaml, id LLM-CHAT-1,
derived_from: story:chat-projection, profile development.driven. No driver run has been launched.
The installed metaharness 0.7.0 supports aep drive run; AEP 0.55.0 installation metadata identifies
source revision 28abe09bb6e5b0a6b4db839f6bf5693957d39324, matching Metaharness's pinned AEP source.

aep doctor passed binary/project/protocol-source/store/plugin checks, warning only that the new
LLM repository has no reachable release tag. The subsequent free task-resolution command refused:

error: the task cannot be resolved: [unknown_protocol] protocol adp/1: no protocol document declares `adp/1` (hint: no protocol documents are loaded at all)

Command: aep govern resolve --task .engineering/task-chat-projection.yaml --format json.
The configured protocol snapshot exists, so investigate the resolver's project/root loading before
a live launch. Do not suppress a guard or guess a map. The inherited maps also name AEP-specific
verification commands; inspect compatibility rather than spending on a known unsuitable map.
Total USD budget and per-session reservation have been requested from the operator and are pending.
The drive skill's refusal rule ends this turn without a launch or fabricated completion evidence.

## ESS behavioral verification

The operator requested ESS specification and behavioral coverage on 2026-09-19. Baseline synthesis
of the declaration-only spec produced zero scenarios. Add a typed routing evaluation observation,
a conformance target calling the real catalog/parser/resolve APIs, authored scenarios for selection
and refusal behavior, complete suite/5 coverage inventory and report/2 evidence. The target records
returned library facts for view assertions; it must not infer an expected outcome from input guards.
Keep ESS test tooling separate from runtime libraries. Pin the conformance library to the same
ESS source as CI. Gate declared coverage, answered counts, named failures and skips, and prove named
scenarios fail after deliberate routing mutations. No provider calls or paid model runs are needed.

## Driver resolution follow-up

Follow-up free diagnosis resolved the unknown_protocol refusal without suppressing a guard.
AEP 0.55.0's explicit --task path uses --root or the current directory rather than the project's
cached protocol source. Passing --root to the configured e27c84bd2f5e565a7974d889ee7e3d27dee872d1
snapshot makes aep govern resolve succeed: LLM-CHAT-1, development.driven, adp/default, 11 obligations.
Retained plan: /home/timo/.cache/llm-planning-20260919/chat-drive-plan.json.
No driver run has launched. The inherited map still needs repository-appropriate verifier review,
and the requested operator total USD budget and per-session USD reservation remain unanswered.
The ESS verifier work is ordinary-session verification; no historical implementation is relabeled
as a governed run. No lifecycle moves were performed beside the driver.
