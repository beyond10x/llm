---
format: aep.planning-md/3
id: architecture-design:llm-boundaries
kind: architecture-design
status: draft
title: Separate protocol, provider, custody and hosting
relations:
- designs: initiative:llm-foundation
- informed_by: specification:declaration-domain
revision: 1
---
## Design

The accepted design is `docs/design.md`. The fourteen compile-only crate boundaries are listed by `cargo metadata --no-deps`. No crate exports a runtime API. Consumers never become dependencies of this workspace.

## Interfaces

Inference accepts an injected SecretResolver and caller-managed renewal facility. Core represents one turn, tools as schemas/results, streaming, usage, capabilities and failures. Routing wraps inference with explicit fallback; protocol code does not select an alternative. Hosting returns a ready endpoint and owned resource identity. Gateway composition shares those implementations.

## Scope exclusions

No agent-loop execution, permission envelope, login ownership, multi-tenancy, cheapest-model optimization, production deployment or consumer migration in this milestone.
