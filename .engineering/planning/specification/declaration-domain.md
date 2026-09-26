---
format: aep.planning-md/1
id: specification:declaration-domain
kind: specification
status: draft
title: Typed declaration model and explicit draft limits
relations:
- specifies: initiative:llm-foundation
revision: 2
---
## Contract

`spec/system.yaml` and `spec/domains/catalog.yaml` declare twelve entities and their reference cardinalities. `ess specify validate --path spec` returned `llm v1 — 2 file(s), valid` on 2026-09-19.

## Limits

This is a declaration/observation model with one terminal Declared state, not runtime conformance evidence. docs/design.md identifies UNMAPPED budget ownership and hosting transitions. Each owning story must refine and validate those semantics before implementation. Protocol, auth and price details remain deliberately unrefined String/record fields until those contracts are specified. No secret value is modeled. Account.secret_reference_id is Optional<String>, with a typed at-most-one credential relation, so explicitly anonymous endpoints are representable. Authenticated modes require a present reference; anonymous mode requires absence and must never invoke the resolver. story:runtime-contracts must encode this conditional invariant in the refined model and story:provider-accounts must test refusal of invalid combinations before any I/O.
