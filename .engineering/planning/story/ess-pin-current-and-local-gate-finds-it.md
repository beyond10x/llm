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
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-11-24T16:24:31Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-24T16:24:32Z", actor: "human:timo", revision: 3}
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
