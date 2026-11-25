---
format: aep.planning-md/3
id: story:ess-pin-current-and-local-gate-finds-it
kind: story
status: active
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
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T21:59:00Z", actor: "human:operator", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T21:59:01Z", actor: "human:operator", revision: 3}
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

Derived 2026-11-25 by `story-scoper` at `87bc16f16`. Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `checks/conformance` and the "Install ess" step of `.github/workflows/gate.yml` — cited
- **Files:** `.github/workflows/gate.yml:96-97` — cited, `ESS_VERSION` and `ESS_SHA256` move together; `:112-113` use the installed `ess`
- **Files:** `checks/conformance/Cargo.toml:40-41` — cited, the `ess-conformance` and `ess-primitives` tags move
- **Files:** `Cargo.lock:537-634` — cited, eight `ess-*` packages at 0.87.0 re-resolve (`ess-compiler`, `-conformance`, `-domain`, `-gen`, `-http`, `-primitives`, `-transport`, `-yaml`)
- **Files:** `checks/conformance/src/gate.rs:11-16` — cited, `Command::new("ess")` takes the first `ess` on `PATH` and reads no version; the version check lands here, before the first projection at `:116`
- **Files:** `contracts/suite.json` — cited, one generated file (135,586 lines, `suite_version` `ess-conformance/35` at `:6221`), regenerated if its bytes move
- **Files:** `contracts/schema/schema/` — cited, 314 tracked generated files (44 commands, 80 entities, 34 events, 156 types); how many change is not known
- **Files:** `contracts/baseline.json` — cited, floors 1068 / 1068 / 0; the acceptance says it must not fall, so it stays as it is
- **Symbols:** `ESS_VERSION`, `ESS_SHA256`, `gate::check`, `gate::ess` — cited
- **Also likely:** `Taskfile.yml:9` — inferred, `ess specify validate --path spec` runs a bare `ess` before the conformance step and outside its refusal
- **Also likely:** `checks/conformance/src/*.rs` (16 files import `ess_conformance` or `ess_primitives`) — inferred, only if the library API moved between 0.87.0 and the new tag
- **Also likely:** `checks/xtask/tests/gate/` — inferred, a test that `gate.yml`, `Cargo.toml` and the documents name one ESS version; `budget.rs:85-106` does this for the nextest pin
- **Also likely:** `checks/xtask/src/main.rs` — inferred, only if the local gate fetches the pinned binary into `target/` (Rust, same digest check as `gate.yml:98-108`) and does not just refuse
- **Documents:** `AGENTS.md:101-108,130-133,150`, `README.md:87`, `CHANGELOG.md` (`[Unreleased]`; precedent at `:129`) — cited, each names 0.87.0 or the pin rule
- **Confidence:** high for the pin sites and the generated files, medium for how the local gate finds the binary: the story leaves "finds it or refuses" open
- **Would collide with:** any unit that regenerates `contracts/suite.json` or `contracts/schema/`, adds a scenario (floor in `contracts/baseline.json`), edits `checks/conformance`, `Cargo.lock`, `gate.yml`, `Taskfile.yml`, or the Gate and Generated files sections of `AGENTS.md`
- **Safety fact:** `ess` is started from one place in Rust, `checks/conformance/src/gate.rs:12`, so a version check there covers both projections; the other two callers are `Taskfile.yml:9` and `gate.yml:112` — unproven
