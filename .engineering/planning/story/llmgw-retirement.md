---
format: aep.planning-md/3
id: story:llmgw-retirement
kind: story
status: draft
title: Deployments move from llmgw to llm-gateway through a reversible cutover; llmgw is archived
relations:
- decomposes: epic:serving-split
- depends_on: story:serving-extraction
revision: 1
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
