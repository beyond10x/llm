---
format: aep.planning-md/3
id: approval-record:wave-2026-10-16-w08-proposal
kind: approval-record
status: draft
title: Wave 2026-10-16-w08 and release 0.7.0 approved by the operator's delegate for this follow-up
tags:
- delegate-approved
relations:
- decides: story:neutral-turn-controls
- decides: story:messages-ingress-client-fields
- decides: story:responses-ingress-client-fields
- decides: story:chat-ingress-closed-levels
- decides: story:client-turns-cross-wire
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-16-w08, and the release stop for 0.7.0.

## What authorises passing it

Approved by the operator's delegate for this follow-up, relayed to this session with the task
(2026-10-16). The approval names the work (the client request fields of llm-gateway 0.9.0's
known limits) and authorises: one commit per unit, bot pushes, the wave pull request and its merge
into `main` once `Gate` and `Shared source gates` are green, and the 0.7.0 release through
AGENTS.md "Release" (release branch, release pull request and its merge, tag `0.7.0`, GitHub
Release `LLM 0.7.0`). Nothing in other repositories.

## The units, and the objective each serves

All five serve `vision:portable-model-inference`, under `epic:client-request-fields`.

- `story:neutral-turn-controls` — llm-core controls, the effort table, `llm.turn/4`.
- `story:messages-ingress-client-fields` — Messages ingress and egress.
- `story:responses-ingress-client-fields` — Responses ingress and egress, the Codex fixtures.
- `story:chat-ingress-closed-levels` — Chat ingress closed at every level, the new options.
- `story:client-turns-cross-wire` — cross-wire acceptance in `checks/conformance`.

## What the store computed

`aep plan artifact waves --kind story --status draft --format json` (aep 0.74.0), after the
scope entries were written: the units fall in layers 1 (`story:neutral-turn-controls`), 2 (the
three wire stories) and 3 (`story:client-turns-cross-wire`). Collisions involving the units:
`story:chat-ingress-closed-levels`, `story:messages-ingress-client-fields` and
`story:responses-ingress-client-fields` each × `story:neutral-turn-controls` on their crate's
`tests` (inferred; ordered by `depends_on`); `story:client-turns-cross-wire` ×
`story:credentials-from-secrets` on `checks/conformance/Cargo.toml` (inferred; the latter is not in
this wave). Unassessed: `story:ingress-block-boundaries` (not in this wave). The three layers run
in this one wave in order: layer 1 merges into `wave/2026-10-16-w08` before layer 2 branches from
it, and layer 3 after layer 2.

## Pre-flight

Primary checkout clean on `main` at 5657d2f5, no other worktree of this repository; 99 GiB free on
`/`. The previous wave measured 3.6 GiB for `cargo test --workspace --no-run` in a fresh tree.
Three implementors at once in layer 2, each in its own managed worktree and `target/`.

## The commits this wave makes

The opening store commit on `wave/2026-10-16-w08`; one unit commit (or a short series) per story
on `impl/<story>`; the merges into the wave branch; the integration commit (contracts, baseline,
CHANGELOG, status, README, AGENTS.md); the closing store commit; the merge into `main` through the
wave pull request. Then the release per AGENTS.md "Release".
