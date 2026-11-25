---
format: aep.planning-md/3
id: story:ess-pin-current-and-local-gate-finds-it
kind: story
status: implemented
title: The ESS pin is the current release and the local gate runs the pinned ess
tags:
- from-finding
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: .github/workflows/gate.yml
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: README.md
- confidence: inferred
  path: Taskfile.yml
- confidence: cited
  path: checks/conformance/Cargo.toml
- confidence: inferred
  path: checks/conformance/src
- confidence: cited
  path: checks/conformance/src/gate.rs
- confidence: inferred
  path: checks/xtask
- confidence: cited
  path: contracts/baseline.json
- confidence: cited
  path: contracts/schema
- confidence: cited
  path: contracts/suite.json
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:00Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-25T07:45:18Z", actor: "human:operator", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

CI installs `ess` 0.87.0, pinned by version and SHA-256 in `.github/workflows/gate.yml`, and
`checks/conformance/Cargo.toml` pins the `ess-conformance` and `ess-primitives` tags with it
(`AGENTS.md`, section "Gate"). On 2026-11-24 the workstation that runs the local gate carried `ess`
0.98.0. The implementor of `story:rubric-judge` reported that `llm-conformance check` fails with
"schema drift" on the untouched base (`main` at `c7c2a62cb`) under 0.98.0, and passes with the 0.87.0
binary whose tarball digest equals `ESS_SHA256`. The coordinator did not re-run that comparison.

So the local gate equals the forge only when the gate is given the pinned binary by hand.

## Outcome

The repository's ESS pin is the current ESS release: `ESS_VERSION` and `ESS_SHA256` in `gate.yml`, the
`ess-conformance` and `ess-primitives` tags, and the regenerated `contracts/suite.json` and
`contracts/schema/`. The local gate finds the pinned `ess` without a manual step, or refuses with the
version it found and the version it needs.

## Acceptance

- `task check` on a clean checkout passes with the `ess` version `gate.yml` names, locally and in CI.
- With another `ess` version on `PATH`, the conformance step names both versions and exits non-zero
  before it compares a projection.
- `contracts/baseline.json` does not fall; no scenario is skipped.

## Verification

The conformance check and `ess specify validate --path spec`. No live call.

## Scope

Read by the coordinator from `git diff --stat` of the unit branch `impl/ess-pin` (head
`140108fcd`) against its base `aa8ba4045`, at the close of wave 2026-11-25-ess-pin: 17 files outside
`contracts/schema/`, and 314 schema files that each lose one `source_digest` line. The
`story-scoper`'s section of 2026-11-25 (read at `87bc16f16`) named `gate.yml`, the manifest,
`gate.rs` and the generated files; it did not foresee a resolver of its own, the guard tests or the
path-list change.

| Path | Scoped | What the unit did |
| --- | --- | --- |
| `.github/workflows/gate.yml` | cited | `ESS_VERSION` "0.99.1" and its `ESS_SHA256`; the step "Specification and plan" validates through `llm-conformance ess`; `AGENTS.md` and `README.md` leave the planning-or-prose path list |
| `checks/conformance/Cargo.toml`, `Cargo.lock` | cited | both `ess-*` tags at 0.99.1, `serde_yaml` for the pin's reader; the eight `ess-*` packages re-locked |
| `checks/conformance/src/ess.rs` | not scoped | new: `Pin::read` from `gate.yml`, `resolve` (the store, `PATH`, the fetch by digest), `Refusal`, `pinned` and `run`; its unit tests |
| `checks/conformance/src/gate.rs` | cited | `check` resolves the pinned `ess` before it projects; `root()` and `project()` take the resolved path |
| `checks/conformance/src/main.rs` | inferred | the subcommand `ess`: every argument after it goes to the pinned `ess` |
| `checks/conformance/tests/client_turns.rs`, `tests/pinned_ess/mod.rs` | not scoped | the process-level cases, a module of the existing test binary |
| `checks/xtask/tests/gate/ess_pin.rs`, `main.rs`, `paths.rs` | not scoped | the guards: one ESS version in every site, no task and no workflow step with an `ess` of its own; the path list's two new rows |
| `Taskfile.yml` | cited | the `ess` step of `check` runs `llm-conformance ess specify validate --path spec` |
| `contracts/schema/` | cited | regenerated with 0.99.1: 314 files lose `source_digest` |
| `contracts/suite.json`, `contracts/baseline.json` | cited | not touched: the suite is byte-identical and the floor stays 1080 |
| `AGENTS.md`, `README.md` | cited | "Gate", "Generated files", the invariants row, the pin-move rule; the version |
| `website/docs/getting-started.md`, `website/docs/guides/run-the-checks.md` | not scoped | the version, and how the local gate gets `ess` |
| `CHANGELOG.md` | inferred | four lines under `### Changed` |

Not touched, as scoped: `spec/`, every scenario, every library crate. The adapters of
`checks/conformance/src/` compile against the 0.99.1 crates unchanged.
