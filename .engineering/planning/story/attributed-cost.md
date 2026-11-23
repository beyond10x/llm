---
format: aep.planning-md/3
id: story:attributed-cost
kind: story
status: implemented
title: A turn's cost is attributed to the model the provider reported
summary: Routes::cost prices the reported model; a missing or different model leaves the cost unknown
relations:
- serves: vision:portable-model-inference
- decomposes: epic:routing
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: crates/llm-cli/tests/command.rs
- confidence: cited
  path: crates/llm-models/src/routes.rs
- confidence: cited
  path: crates/llm-models/tests/routes_toml_bedrock.rs
- confidence: cited
  path: docs/design.md
- confidence: inferred
  path: docs/implementation-status.md
- confidence: inferred
  path: docs/pricing.md
- confidence: inferred
  path: website/docs/guides/use-the-llm-command.md
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-11-23T01:50:11Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}}
- {from: "proposed", to: "active", at: "2026-11-23T01:50:14Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}}
- {from: "active", to: "implemented", at: "2026-11-23T01:50:15Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1}}}
---
## Context

Review of 0.24.0 (finding 9): in three Messages turns the provider reported the configured model,
a different model, or no model, and `llm infer` printed the same `cost=0.000032 USD` for 12 input
and 4 output tokens in all three. `Routes::cost` (`crates/llm-models/src/routes.rs`) gave
`llm-cost` the route's configured `upstream_model` as the attempt's model, so the quote always
matched the route price. `docs/pricing.md` ("Model attribution, cache partitions and unknowns")
requires the actual model the producer reported: a missing or different reported model produces
named unknown lines. `docs/design.md` said a route's `price` applies to the model it declares
without saying which model a turn is attributed to.

## Acceptance

- `Routes::cost` prices a turn on a priced route under the model the turn's observation reports:
  when it is the route's declared model, the complete total is the route-price total (a Messages
  turn of 12 input and 4 output tokens at 1 and 5 USD per million: `0.000032`).
- When the observation reports a different model, or no model, the complete total is absent and
  the cost carries unknown lines (`model-mismatch`, `model-unknown`); the configured model's price
  is never the attributed cost.
- `llm infer` on a priced Messages route prints `cost=0.000032 USD` on its stderr usage line when
  `message_start` reports the route's model, and `cost=unknown` when it reports another model or
  none; stdout and exit code are unchanged.
- No configured-rate estimate is printed or returned in place of the attributed cost.
- `docs/design.md` states that a route price is attributed to a turn only when the turn reports
  the route's model; `docs/pricing.md` and the guide page agree with the printed line.

## Evidence

Fixture tests only (fixture rate card, loopback server, no network, no model call):
`crates/llm-models/tests/routes_toml_bedrock.rs` and `crates/llm-cli/tests/command.rs`.
