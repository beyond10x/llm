---
format: aep.planning-md/3
id: epic:serving-split
kind: epic
status: draft
title: Split serving out of llm into llm-gateway; Harness builds on llm
relations:
- decomposes: initiative:llm-foundation
revision: 2
---
## Outcome

`beyond10x/llm` is the client library only. The serving side moves to a repository named
`llm-gateway`, and Harness can build against llm's client crates in place of its own wire crates,
with llm covering at least every capability those crates have.

## Why

Operator request, 2026-10-05: "llm -> extract out of it llm-gateway (clearer naming) - then make sure
harness can technically use that llm crate and not the internal one. make sure llm is at least as
good as harness/llm".

The repository review of 2026-10-04 found llm holding two products: a client SDK and a serving
stack (gateway, hosting port, GPU provisioning). The serving crates import nothing from the client
crates (`grep 'use llm_'` over their `src`: only `llm-runpod` uses `llm_provision`), so the cut is
clean at crate level.

## Crates on each side (src lines, `git ls-files … | wc -l`, llm `3ee18e44`)

| Moves to llm-gateway | Lines | Stays in llm | Lines |
|---|---|---|---|
| `llm-gateway` | 1,944 | `llm-core` | 1,068 |
| `llm-provision` | 2,781 | `llm-messages` | 1,456 |
| `llm-runpod` | 2,037 | `llm-responses` | 1,212 |
| `llm-modal` | 5 | `llm-chat` | 1,539 |
| | | `llm-http` | 453 |
| | | `llm-credentials` | 644 |
| | | `llm-providers` | 424 |
| | | `llm-routing` | 812 |
| | | `llm-cost` | 1,801 |
| | | `llm-cli` | 5 |

`llm-routing` and `llm-cost` stay: routing and the cost ledger run in the caller's process.

## Harness comparison

Harness's own model crates: `harness-messages`, `harness-responses`, `harness-http`,
`harness-credential`, 7,528 src lines together (harness `2fd7235b`). Their users inside Harness:
`harness-cli`, `harness-app-server` and each other (`grep` over `crates/*/Cargo.toml`).

## Stories

- `story:serving-extraction`: move the four serving crates and their plan to `llm-gateway`.
- `story:harness-parity`: a cited capability matrix, Harness wire crates against llm.
- `story:harness-builds-on-llm`: Harness builds and passes its tests on llm in place of its wire
  crates, on a branch, with no cutover.
- `story:llmgw-retirement`: deployments move from `llmgw` to `llm-gateway`; `llmgw` is archived
  (operator decision 2026-10-05, option B).

## Not in scope

Moving Harness or Metaharness onto llm's client crates in production; retiring Harness's wire
crates.
