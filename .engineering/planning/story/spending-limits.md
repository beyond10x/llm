---
format: aep.planning-md/2
id: story:spending-limits
kind: story
status: implemented
title: Spending admission accounts for concurrency and uncertainty
relations:
- decomposes: epic:routing
- depends_on: story:usage-pricing
- depends_on: story:catalog-routing
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: .github/workflows
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: README.md
- confidence: cited
  path: Taskfile.yml
- confidence: cited
  path: checks/conformance
- confidence: cited
  path: contracts
- confidence: cited
  path: crates/llm-cost
- confidence: cited
  path: docs
- confidence: cited
  path: docs/budgets.md
- confidence: cited
  path: docs/verification/budgets.md
- confidence: cited
  path: spec
- confidence: inferred
  path: spec/domains/catalog.yaml
revision: 14
---
## Context

Resolve spending admission for the accepted single-owner deployment: reservations, settlement,
provider-accepted ambiguity, persistence/restart, fixed subscription versus marginal charges and
compute shutdown obligations. The validated ESS budget domain and docs/budgets.md now record this
contract. Estimate-based policy is explicit and makes no hard provider-invoice ceiling claim.

## Acceptance

A deterministic ledger simulation demonstrates that concurrent requests and provisioned resources cannot bypass the declared spending-admission policy, including unknown usage and restart.

## Evidence

Implemented the pure BudgetEngine and optional SqliteLedger in crates/llm-cost/src/budget.
The immutable policy, exact wide totals, one-shot dispatch receipts, unknown/partial settlement,
fixed-charge observations, compute renewal/stop obligations, process ownership and replay rules
are documented in docs/budgets.md. A failed journal write freezes admission until verified reopen
and preserves known cleanup obligations for inspection.

Full task check passed: 75 behavior tests plus one subprocess fixture entry point, two compile-fail
documentation tests, formatting, strict Clippy, feature checks, ESS and AEP. Three consecutive
restored-source ESS runs each passed all 145 scenarios (140 authored, five generated), zero
failed/error/unsupported/skipped/refused. All 84 schemas and original suite bytes regenerate
exactly. Six prior immutable review records retain missing-findings warnings.

All 16 deliberate budget defects were caught by named authored scenarios and every source restored
byte-for-byte. See docs/verification/budgets.md, budget-report.json and budget-falsification.json.
Cross-process lock contention, abrupt exit and a real failed SQLite COMMIT are additionally tested.
No paid call or deployment occurred. Gateway/fallback/hosting integration and live qualification
remain outstanding; these library results do not claim actual cloud shutdown or a hard invoice
ceiling. Remote publication/CI is tracked separately at the exact candidate commit; see Native CI correction.

## Verification

Retain commands and exact fixture/contract identities demonstrating the acceptance and its named failure cases. A passing scaffold build is not runtime evidence. No paid call runs in the normal gate.

## Scope

Cited implementation surfaces: crates/llm-cost, Cargo.lock, spec, checks/conformance, contracts,
Taskfile.yml, .github/workflows, README.md and docs. The budget domain resolves the earlier catalog
UNMAPPED budget scope; the existing catalog path remains historical context. Shared specification
and workspace manifests are integration surfaces; coordinate changes through the owning story.

## Implementation contract

One immutable, explicitly configured ledger belongs to the accepted single-owner deployment. It
covers all admitted model attempts and owned compute in that scope, in one currency, with an exact
monetary cap and concurrent-obligation limit. The validated ESS budget domain owns its reservation
and posted-charge records and references accounting's recorded-charge observations. No automatic
period reset, replacement ledger or cross-currency conversion is allowed. Administrative selection
of another ledger is a different policy scope, not a reset hidden behind reopening.

Implement an I/O-free deterministic engine plus an optional SQLite journal adapter. The adapter
holds an OS lock for its lifetime, refuses a second process owner, and serializes concurrent callers.
Use explicit create versus open-existing; immutable policy mismatch, missing/corrupt/unsupported
journals and persistence failures refuse. Commit before returning dispatch authority. Preserve the
journal and replay recorded decisions on reopening; only then persist recovery transitions.
Default Rust library users do not acquire a database dependency or storage ownership.

A reservation pins identity, attribution, declared estimate/assumption and expiry. Begin consumes
its right to start once, before the external effect; duplicate IDs or repeated begin never grant
another permit. Cancel releases only a never-started reservation. Unknown/ambiguous results and
recovered starts retain their reservation and block new admission until reconciled. Compute keeps
a stop-required obligation until shutdown is evidenced, then retains unresolved cost until settled.
Expiry is not evidence of shutdown or no charge. Renew atomically adds a declared estimate and
extends time; a failed extension requires stopping a started compute resource. Clock reversal
refuses, and elapsed obligations are processed even when the subsequent request is denied.

Settlements preserve known subtotals and completeness. An actual or estimated charge above its
reservation is recorded even if it exceeds the cap; later admission stops. Aggregate nanounits
use checked wide integers, never floats, saturation or dropped overrun evidence. Recorded fixed
subscription/compute/metered charges enter once by stable observation ID, separately from reserved
attempts. A reference token valuation is not an extra subscription/self-hosted charge. Callers must
reconcile an invoice with its estimate instead of posting both as independent obligations.

These are declared-estimate admission limits, not a guaranteed ceiling on a provider's invoice.
Callers own forecast bounds, evidence accuracy, stable IDs and routing every paid effect through
the configured ledger. The future gateway/hosting integration must consume permits and stop
obligations; a library simulation alone does not prove real cloud shutdown. Trusted local storage
is required; malicious administrators or rollback of a database backup are outside this contract.

ESS declarations explicitly describe inspection records and closed phase values, rather than
inventing a production event transport. Add stateful authored command sequences against the real
engine and SQLite adapter, inspect actual totals/phases/permits, and use mutation checks on
admission, unknowns, one-shot start, recovery, settlement overruns and compute stop obligations.
Preserve the current 97 scenarios. No paid calls, deployments or artifact lifecycle moves occur.

## Native CI correction

The initial expanded native CI run, 35439902423 at 183a8a1, failed macOS's reopen/policy-mismatch
assertion; the original fail-fast matrix cancelled Windows. Investigation reproduced OwnerBusy
on Linux by retaining a duplicate of the owner's lock descriptor across ledger drop, matching
standard-library documented lock lifetime behavior. The new Unix regression failed before the
fix. OwnerLock now explicitly unlocks after connection destruction, so a descriptor briefly
inherited during concurrent process spawn cannot retain the cooperative owner lock after close.
The assertion now reports the actual error, and CI collects both native platform results.

The full local gate was rerun after the fix. It passes 75 behavior tests plus one subprocess
fixture entry point and two compile-fail documentation tests. Three restored-source ESS runs
retain 145/145 passes, zero failed/error/unsupported/skipped/refused and 84 matching schemas. All
16 deliberate budget defects again fail named scenarios; every mutation is restored. The report
and source identities are refreshed. Run 35440347352 at d28dfc6 passed macOS and all Windows runtime tests, but Windows Clippy
refused the always-successful non-Unix directory-sync helper. The helper and its calls now
compile only on Unix, leaving Windows on SQLite native persistence. A fresh exact-revision
remote run must verify the complete macOS/Windows gate;
no timing allowances, skipped tests or exception lists were introduced.
