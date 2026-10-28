---
format: aep.planning-md/3
id: story:routes-refusal-names-a-later-route-field-line
kind: story
status: proposed
title: A routes refusal names the line of an unknown field in any route
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
revision: 2
transitions:
- {from: "draft", to: "proposed", at: "2026-10-28T13:16:04Z", actor: "human:timo", revision: 2}
---
## Context

Found by the adversary of `story:routes-refusal-test-names-its-field` in wave 2026-10-20-w10
(`review-result:adversary-w10-routes-test-pass-1`), present at the base `5e00283d`. `RawRoute` is
`#[serde(untagged)]` (`crates/llm-models/src/routes.rs:122`): a route is either a model string or a
mapping read as `RawDeclaredRoute`. An untagged enum's error carries no position, so serde_yaml
0.9.34 stamps it with the start of the `routes:` mapping. An unknown field in a block-style route,
or in any route after the first, is refused at that line rather than at the field's. Two cases that
show it are kept in the wave's scratch (`red-route-line-cases.rs`): the unit's ROUTE case with one
route before it is refused at line 5 for a field on line 6; a block route with `region: eu` on line
7 is refused at line 5. The crate's own test `DOCUMENT` declares a route third, in block style.

## Outcome

A routes refusal for an unknown route field names the line that holds the field, wherever the route
sits in the mapping.

## Acceptance

- The two cases pass: a later route and a block route are each refused at the field's own line.
- The ROUTE case of `no_refusal_is_classified_by_the_text_of_the_field_it_names` gains a route
  before it, so its line check can fail.
- The refusal text still quotes no secret reference and no provider prose.
- A suggested shape, not binding: a visitor that reads a string as the model and passes a mapping
  through `serde::de::value::MapAccessDeserializer` into `RawDeclaredRoute`, so the field's key keeps
  its position.
