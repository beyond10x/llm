---
format: aep.planning-md/3
id: epic:hosting
kind: epic
status: archived
title: Provisioned inference
relations:
- decomposes: initiative:llm-foundation
- informed_by: specification:declaration-domain
revision: 3
transitions:
- {from: "draft", to: "archived", at: "2026-10-05T10:17:37Z", actor: "human:timo", revision: 3}
---
## Outcome

Provisioned inference supplies its part of the full foundation described in docs/design.md.

## Done when

All child outcomes have retained verification evidence and their contracts are included in story:foundation-qualified; the existing compile-only scaffold is not completion evidence.

## Evidence

Operator direction 2026-09-19; docs/design.md; spec/system.yaml.

## Moved

Moved to `beyond10x/llm-gateway` as `epic:hosting` on 2026-10-05 by story:serving-extraction, with
the crates it describes (llm-gateway `be722b4`). Archived here; the work continues there. Its
evidence records stay in this store.
