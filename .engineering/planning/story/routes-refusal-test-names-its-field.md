---
format: aep.planning-md/3
id: story:routes-refusal-test-names-its-field
kind: story
status: draft
title: The routes refusal test is refused for the field each case names
summary: The ROUTE case passes on a YAML error in the auth line because the helper's AUTH default contains ROUTE
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
revision: 1
---
## Context

Found by the implementor of story:capabilities-declare-streaming in wave 2026-10-20-w09. In
`crates/llm-models/tests/routes.rs`, the test helper `single()` has a default `AUTH` value that
contains the text `ROUTE`. The `ROUTE` case of
`no_refusal_is_classified_by_the_text_of_the_field_it_names` therefore also rewrites the secret
reference, and the case passes on a YAML error in the auth line, not on the route field it means to
test.

## Outcome

Each case of that test is refused for the field it names.

## Acceptance

- The `ROUTE` case fails when the routes parser stops refusing the route field it names (checked
  by a mutation), and passes again when it refuses it.
- The helper's default values contain none of the field markers the test substitutes.
