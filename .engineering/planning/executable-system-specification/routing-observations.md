---
format: aep.planning-md/1
id: executable-system-specification:routing-observations
kind: executable-system-specification
status: draft
title: Executable catalog routing observations
relations:
- verifies: story:catalog-routing
- verifies: story:provider-accounts
model_digest: 4e44a980c955cfdd58c4e198cd2de07dbbd9b3cf3ee289f6652a38cadbc88e2f
revision: 2
---
## Scope

Spec: spec/system.yaml, spec/domains/catalog.yaml and spec/domains/routing.yaml.
Runtime declarations plus the explicit verification adapter observation contract; no claim of
live inference, durable budgets or hosting lifecycle qualification.

## Verification

The declaration baseline generated zero scenarios. Complete suite/5 now includes 27 authored
behavior checks and one generated adapter check. The real libraries answer all 28, with zero
failures/errors/unsupported/skips/refusals in three consecutive local runs. See
contracts/routing/suite.json, docs/verification/routing-report.json and
 docs/verification/routing-conformance.md. Eight restored production mutations fail named scenarios.

The report source identity hashes runtime/checker Rust, manifests and Cargo.lock. CI uses the same
pinned ESS library and CLI, regenerates schemas/suite and checks the report rather than only an
exit code. Runtime provider and hosting qualification remains separate and outstanding.

No artifact lifecycle transition is performed beside the requested governed driver. This record
retains executable evidence without claiming the entire foundation or its release is complete.
