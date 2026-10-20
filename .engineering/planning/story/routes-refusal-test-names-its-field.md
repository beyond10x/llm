---
format: aep.planning-md/3
id: story:routes-refusal-test-names-its-field
kind: story
status: implemented
title: The routes refusal test is refused for the field each case names
summary: The ROUTE case passes on a YAML error in the auth line because the helper's AUTH default contains ROUTE
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: crates/llm-models/tests/routes.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T07:47:11Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-20T07:47:11Z", actor: "human:timo", revision: 5}
- {from: "active", to: "implemented", at: "2026-10-20T13:49:44Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
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

## Scope

Derived 2026-10-20 by `story-scoper` at 7873aa63 (tag 0.8.0). Every line is **cited** (read from the story or the tree) or **inferred** (a reading that could be wrong).

- **Primary surface:** `crates/llm-models` tests only — cited
- **Files:** `crates/llm-models/tests/routes.rs` — cited
- **Symbols:** `single` (`:53-73`) and its default `AUTH` (`:58-61`); `SINGLE` (`:41-51`); `no_refusal_is_classified_by_the_text_of_the_field_it_names` (`:261-295`), `ROUTE` case `:275-278` — cited
- **Defect site:** `single` applies `str::replace` per placeholder (`:70`), `ROUTE` last (`:64`), so it rewrites the `ROUTE` inside `LLM_ROUTES_TEST_NEVER_SET` on document line 7 (`:47`); the case is refused for a YAML syntax error in the auth line, which the `starts_with(... "at line ")` assertion (`:286-292`) accepts — cited (read, not run)
- **Also likely:** the `ROUTE` case's `capabilities: {}` (`:277`) needs the seven required fields (`crates/llm-core/src/vocabulary.rs:50-57`; `LIMITS` at `:425`) or the mutation check cannot fail; the `NEVER_SET` non-echo assertions at `:676` and `:814` are vacuous today because `single(&[])` writes `env:LLM_p/mS_TEST_NEVER_SET` — inferred
- **Documents:** none — inferred
- **Confidence:** high — cited
- **Would collide with:** any unit editing `crates/llm-models/tests/routes.rs` (`single` has 38 call sites in the file) — cited
- **Safety fact:** `single` and `SINGLE` exist only in this file, so changing the defaults moves no other test; the callers relying on the default `AUTH` (`:404`, `:669-676`, `:796-814`) must still pass — unproven
