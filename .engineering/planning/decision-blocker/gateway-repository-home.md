---
format: aep.planning-md/3
id: decision-blocker:gateway-repository-home
kind: decision-blocker
status: cleared
title: New llm-gateway repository, or rename llmgw to llm-gateway
relations:
- blocks: story:serving-extraction
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-04T22:20:04Z", actor: "human:timo", revision: 3}
---
## Question

Where do the serving crates go: a new repository `beyond10x/llm-gateway`, or the existing
`beyond10x/llmgw` renamed to `llm-gateway`?

| Option | Does | Costs |
|---|---|---|
| A | Rename `llmgw` to `llm-gateway` and move the four crates into it | One gateway repository; llmgw's own 2,857 lines and llm's 6,767 serving lines must be reconciled in one tree |
| B | New repository `llm-gateway`; llmgw retires after a cutover | Two gateways until the cutover; the llm README already plans that cutover |

Recommendation at filing: A.

## Sources

llmgw `README.md:1-3`, llmgw `048ebd8`; llm `README.md:27`.

## Decision

B, the operator, 2026-10-05: "gateway -> B". A new repository `beyond10x/llm-gateway` takes the four crates; `beyond10x/llmgw` stays in service and retires after a qualified cutover (`story:llmgw-retirement`).
