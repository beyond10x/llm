---
format: aep.planning-md/2
id: executable-system-specification:routing-observations
kind: executable-system-specification
status: draft
title: Executable foundation library observations
relations:
- verifies: story:catalog-routing
- verifies: story:provider-accounts
- verifies: story:local-secret-adapters
- verifies: story:usage-pricing
- verifies: story:spending-limits
- verifies: story:neutral-inference
- verifies: story:runtime-contracts
model_digest: 1e4410acbb59104a27f7fbdde1f33de64703f1b86743bfd6ff773fcc1d5ca823
revision: 12
---
## Scope

Spec: spec/system.yaml and its catalog, routing, secrets, accounting, budget and inference domains.
The stable artifact ID retains its original routing name. Observers execute the real Rust libraries
and optional disposable SQLite journals. No credential-custody service, invoice authentication,
live provider/client qualification or cloud shutdown is claimed.

## Verification

Implemented bound TurnObservation in successful outcomes and optional boxed failure evidence.
Actual upstream model/response IDs stay absent when unreported; partial/final usage is independent
of success. Validation rejects foreign binding coordinates, contradictory usage and unsent evidence.
Unreleased envelope versions are turn/2 (unchanged), outcome/3, usage/2 and cost/2. Partial prices
retain known lower bounds and never complete a total, including fully populated/all-zero snapshots.

Verification: task check exits 0; 183/183 ESS scenarios pass three consecutive runs, 177 authored and
six generated, zero failed/error/unsupported/skipped/refused; all 89 schemas regenerate exactly.
Ten deliberate production mutations fail named scenarios, restored byte-for-byte. See
 docs/verification/observations.md, observations-report.json and observations-falsification.json.
Source identity: llm-foundation-libraries sources-sha256:372762ca11728e8cdccf7fe8fc82d04f207ecb99485d9a116a868aa4295c4f2e
Spec digest: 1e4410acbb59104a27f7fbdde1f33de64703f1b86743bfd6ff773fcc1d5ca823
Suite: sha256:0d7fe71de43ee8f7dd9ed3b41f939da99725886e045b12f10db105b999596f6e

No lifecycle moves beside the pending Chat driver. No provider/gateway/hosting integration or
remote qualification is claimed by this checkpoint; Messages implementation follows.
