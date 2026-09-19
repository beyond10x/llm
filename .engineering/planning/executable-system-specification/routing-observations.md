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
- verifies: story:spending-limits
model_digest: f84d77e1dea49c2858b17cc6e28070d6473733e85f661c753c04886ad42e19e7
revision: 10
---
## Scope

Spec: spec/system.yaml and catalog, routing, secrets, accounting and budget domains. The stable
artifact ID retains its original routing name. The compiled model now includes single-owner
spending admission and verification observations over the real optional SQLite adapter. No live
invoice accuracy, credential-custody service or cloud shutdown qualification is claimed.

## Verification

Complete suite/5 at contracts/suite.json contains 140 authored scenarios and five generated checks.
All 145 pass with zero failed/error/unsupported/skipped in three consecutive restored-source local
runs; synthesis has zero refusals. All 97 preceding pricing/routing/secret checks remain. The gate
regenerates 84 schemas and the original suite, compares exact bytes/file sets and gates actual
report/2 counts after readmission against that suite. See docs/verification/budgets.md and its
paired budget-report.json. All 16 budget mutations fail named scenarios; prior mutation and
report records remain paired to their historical revisions.

The target exposes production quote facts, route explanations and secret results, plus budget
policy, totals, errors, permits, phases, shutdown obligations and attribution preservation. Budget
programs operate disposable SQLite journals through production APIs, including concurrency, reopen
and injected storage failures. Expectations are independent fixture values; the target never reads
scenario names or assertions. Runtime tests also exercise another process and abrupt process exit.
No runtime crate depends on ESS. Reports identify full runtime/checker sources and real timestamps.
CI runs the same gate and retains exact artifacts; native credential and local ledger tests run on
macOS and Windows. No artifact lifecycle moves are performed beside the pending Chat driver.
