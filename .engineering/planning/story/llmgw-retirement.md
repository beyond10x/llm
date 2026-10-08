---
format: aep.planning-md/3
id: story:llmgw-retirement
kind: story
status: draft
title: Deployments move from llmgw to llm-gateway through a reversible cutover; llmgw is archived
relations:
- decomposes: epic:serving-split
- depends_on: story:serving-extraction
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: website/data/status.json
- confidence: inferred
  path: website/docs/status.mdx
- confidence: inferred
  path: website/docs/status/roadmap.md
revision: 3
---
## Outcome

Every deployment served by `beyond10x/llmgw` is served by `beyond10x/llm-gateway`, through a
qualified cutover that can be reversed, and `llmgw` is then archived.

## Why

Operator decision 2026-10-05 ("gateway -> B"): a new `llm-gateway` repository, with `llmgw`
retiring after a cutover. llm `README.md:27` already required the cutover to be "qualified" and
"reversible".

## Acceptance

- A capability matrix of `llmgw` (its routes, backends, registry, auth, scale-to-zero) against
  `llm-gateway`, each row citing both sides; no row uncovered.
- Harness and Metaharness, the clients `llmgw` `README.md` names, run against `llm-gateway` with
  one URL and one token, each with a recorded run.
- A documented rollback to `llmgw`, exercised once.
- `beyond10x/llmgw` archived on GitHub by the bot, and its README points at `llm-gateway`.

## Depends on

`story:serving-extraction`.

## Scope (inferred)

llm-gateway repository; llmgw `README.md`; deployment configuration of Harness and Metaharness.

## Scope

Scoped 2026-10-08 at 560f044c; confidence high.

- The acceptance lands outside this repository: the llm-gateway cutover and rollback, the llmgw archival, and the Harness and Metaharness deployment configuration.
- In this repository only status bookkeeping changes at close: `docs/implementation-status.md` (`:41`, `:120`), `website/docs/status/roadmap.md:20`, then `task docs-generate`; optionally `docs/design.md:11`, `:87` and `CHANGELOG.md`.
- No code depends on llmgw; the only mention is the forbidden-name check at `crates/llm-core/tests/dependency_boundary.rs:27`, which archival leaves true.
- The Why section's citation of `README.md:27` is stale: that line now says serving moved to llm-gateway in 0.2.0.
