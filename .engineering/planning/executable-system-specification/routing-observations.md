---
format: aep.planning-md/1
id: executable-system-specification:routing-observations
kind: executable-system-specification
status: draft
title: Executable foundation library observations
relations:
- verifies: story:catalog-routing
- verifies: story:provider-accounts
- verifies: story:local-secret-adapters
- verifies: story:usage-pricing
model_digest: cbb0c09cdeb2d984d37bf4f39eea107cd24d7ab1ff10a7fcc4598c7570a592cf
revision: 8
---
## Scope

Spec: spec/system.yaml and catalog, routing, secrets and accounting domains. The stable artifact ID
retains its original routing name; the compiled model now includes pure usage/pricing concepts and
verification observations as well as routing and local secret resolution. No live invoice accuracy,
persistent credential store, durable budget or hosting qualification is claimed.

## Verification

Complete suite/5 at contracts/suite.json contains 93 authored scenarios and four generated checks.
All 97 pass with zero failed/error/unsupported/skipped in three consecutive restored-source local
runs; synthesis has zero refusals. All 52 preceding routing/secret checks remain. The gate
regenerates 69 schemas and the original suite, compares exact bytes/file sets and gates real
report/2 counts after readmission against that suite. See docs/verification/pricing.md and the
paired pricing-report.json. Eleven pricing mutations fail named scenarios; earlier mutation and
report records remain paired to their historical revisions.

The target exposes production quote quantities, amounts, unknown reasons, attribution preservation,
price source/revision/identity and separate totals. It never reads scenario names or assertions.
Actual amounts and canonical price identities are independently expected by the authored fixtures.
No runtime crate depends on ESS. Reports identify the full runtime/checker source and real time.
CI runs the same gate and retains exact artifacts; native credentials compile separately on
macOS and Windows. No artifact lifecycle moves are performed beside the pending Chat driver.
