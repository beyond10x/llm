---
format: aep.planning-md/3
id: epic:connectors-secret-adapter
kind: epic
status: draft
title: Resolve stable secret references through future Connectors custody
relations:
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

Add an optional SecretResolver implementation after Connectors releases independent arbitrary-secret lookup. No Connectors dependency in core and no new reference format in routing. This is outside the first milestone.

## Evidence

Operator explicitly deferred this integration on 2026-09-19; docs/design.md, Credentials and configuration.
