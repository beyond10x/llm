# Budget ledger verification — 2026-09-19

The [budget contract](../budgets.md) defines the implemented single-owner admission policy and its
limits. This checkpoint adds an optional real SQLite journal; it makes no model or hosting calls.

## Counted coverage

The combined ESS suite has **145 scenarios**: 140 authored and five generated, with zero synthesis
refusals. It retains all 97 preceding pricing/routing/secret scenarios and adds 47 stateful budget
programs plus one generated observation check. Three consecutive restored-source runs each pass
145 scenarios with zero failed, error, unsupported or skipped observations. The baseline holds
answered/total floors at 145 and a skipped ceiling of zero. No case was quarantined or removed.

The [report](budget-report.json) is paired with [the exact suite](../../contracts/suite.json).
The gate regenerates and byte-compares that suite and all 84 schemas, admits original suite/5 bytes,
runs the pinned ESS 0.26.0 runner against the real libraries, re-admits report/2 and gates complete
coverage and counts. Source/specification/suite identities and real timestamps are retained.

The budget target creates disposable local SQLite journals, invokes public production APIs, and
observes returned errors, permits, policy, amounts, phases and attribution. Its programs include
real reopen, simultaneous threads, another owner handle and explicitly injected database faults.
It never reads scenario names or expected assertions to decide an answer. Fixture expectations
are independently stated exact amounts and phases, including totals beyond u64 and known partial
overruns. The notification and execution-record entities are verification plumbing, not a claim
that production publishes an event bus or uses an Entity Runtime store.

## Runtime/storage checks

The Rust checks additionally exercise real cross-process lock contention, abrupt child-process
exit without destructors, and a deferred SQLite constraint that fails COMMIT after INSERT succeeds.
No permit escapes a failed commit. The failed owner stays closed to admission until verified
reopen; known cleanup obligations remain inspectable. Concurrent callers share one cap. Existing
policies, corrupt journals and missing storage never silently create a fresh budget.

There are 12 new behavior tests and one subprocess fixture entry point, which the process test
invokes in two modes. Together with the prior foundation, this is 74 behavior tests plus that
entry point, and two compile-fail documentation tests. The full `task check` also checks default
and optional features, formatting, strict Clippy, ESS and AEP. Six preexisting immutable review
records retain missing-findings warnings. CI additionally runs credential and local ledger tests
on macOS and Windows; verify those exact jobs before claiming their remote results.

## Falsification

All 16 deliberate production defects fail a named authored budget scenario, with zero target
errors, unsupported observations or skips. Every source was restored byte-for-byte. The
[retained record](budget-falsification.json) includes exact edits, source hashes and outcomes.

| Defect | Named failure | Failed / passed |
| --- | --- | --- |
| `omit-held-reservations` | `cap-includes-other-reservations` | 3 / 142 |
| `ignore-unknown-spend` | `unknown-holds-reservation` | 5 / 140 |
| `reissue-start` | `one-shot-start` | 2 / 143 |
| `expire-start-as-free` | `expired-start-is-not-free` | 1 / 144 |
| `expire-compute-as-free` | `compute-expiry-is-not-shutdown` | 1 / 144 |
| `forget-recovery-uncertainty` | `restart-retains-interrupted-model` | 4 / 141 |
| `omit-renewal-stop` | `failed-renewal-keeps-stop-obligation` | 3 / 142 |
| `settlement-overrun-is-capped` | `settlement-overrun-is-retained` | 4 / 141 |
| `ignore-failed-commit` | `failed-commit-issues-no-permit` | 3 / 142 |
| `ignore-owner-lock` | `second-owner-refuses` | 1 / 144 |
| `ignore-policy-mismatch` | `reopen-refuses-policy-reset` | 1 / 144 |
| `allow-after-write-failure` | `write-failure-freezes-owner-until-reopen` | 3 / 142 |
| `ignore-clock-reversal` | `clock-reversal-refuses` | 1 / 144 |
| `allow-wrong-settlement-currency` | `settlement-currency-refused` | 1 / 144 |
| `round-wide-exposure` | `wide-aggregate-charge-is-exact` | 2 / 143 |
| `allow-concurrency-overflow` | `concurrency-includes-unfinished-work` | 1 / 144 |

## Qualification boundary

These fixtures prove the implemented library policy over trusted forecasts, clocks and evidence.
They do not prove physical power-loss behavior, truthful provider invoices, arbitrary network
filesystems, protection against privileged journal replacement, or real cloud shutdown. The
engine's copy/replay costs and lack of automatic compaction are documented; no throughput benchmark
is claimed. Gateway/fallback/hosting integration must still consume permits and cleanup obligations.

CI retains `foundation-conformance`. Verify the exact published commit, all required jobs, original
suite/schema bytes, all three reports and nonempty detailed checks before reporting remote
completion. A green job alone is not the evidence. Full foundation qualification, release and
consumer migrations remain outstanding.
