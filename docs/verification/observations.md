# Bound inference observations — 2026-09-19

Successful outcomes and failures can now retain selected binding, actual upstream model and response
IDs, and reported usage independently of execution success. Partial usage stays an incomplete lower
bound in pricing. See [the core contract](../contract-v1.md) and [pricing](../pricing.md).

`task check` exits 0 after restoring every mutation. The combined suite has **183 scenarios**:
177 authored and six generated. It retains all 145 preceding cases and adds 32 inference scenarios,
five accounting scenarios and one generated observation. Three consecutive runs pass all 183 with
zero failed, error, unsupported or skipped. Synthesis has zero refusals. The baseline holds total
and answered floors of 183 with a skipped ceiling of zero; no cases were removed or quarantined.
The gate byte-compares the original suite and all 89 schemas against fresh ESS projections.

The [report](observations-report.json) pairs with [the suite](../../contracts/suite.json). Reports
from earlier checkpoints remain historical evidence paired to those published revisions, not to
this expanded suite. This implementation adds three Rust behavior tests: the foundation now has
78 behavior tests, one subprocess fixture entry point and two compile-fail documentation tests.
Default/optional features, formatting, strict Clippy, ESS and AEP all pass locally. AEP retains six
preexisting immutable review records with missing-findings warnings. Native CI must separately
verify the exact published revision; this local record makes no remote completion claim.

The inference observer deserializes actual production values, runs their real binding/finality/
usage validation and round-trips accepted values. Its view exposes returned model and response IDs,
all five optional counters as exact decimal strings, finality, errors and dispatch. It reads neither
scenario names nor assertions. Accounting scenarios call the production quote APIs and observe
quantities, amounts, incomplete reasons and totals. Explicit expectation values are independent of
the target. The notification/view entities are verification plumbing, not a production event bus.

The first authored success payloads encoded StopReason as a string; the public codec uses a tagged
object. The gate reported 13 failures (zero errors/skips). Correcting those fixture payloads to the
existing producer format made their acceptance and semantic-refusal assertions execute. No runtime
contract was relaxed to admit the mistaken fixtures.

## Falsification

All ten deliberate production defects fail named scenarios, with zero target errors, unsupported
observations or skips. Every changed source was restored byte-for-byte before the final full gate.
The [record](observations-falsification.json) retains exact edits, source hashes and all failures.

| Defect | Named failure | Failed / passed |
| --- | --- | --- |
| `allow-foreign-observation` | `foreign-account-refused` | 12 / 171 |
| `allow-partial-success` | `partial-success-refused` | 1 / 182 |
| `allow-unsent-evidence` | `unsent-observation-refused` | 1 / 182 |
| `lose-failed-observation` | `failure-retains-partial-observations` | 12 / 171 |
| `invent-upstream-model` | `unknown-identifiers-and-usage-stay-absent` | 2 / 181 |
| `unknown-cache-becomes-zero` | `wide-count-and-zero-stay-exact` | 1 / 182 |
| `partial-becomes-final` | `partial-usage-is-lower-bound` | 3 / 180 |
| `total-ignores-partial` | `partial-zero-is-not-final` | 3 / 180 |
| `discard-partial-lower-bound` | `partial-usage-is-lower-bound` | 3 / 180 |
| `failure-invalidates-final-usage` | `failed-ambiguous-attempt-is-counted` | 1 / 182 |

## Limits

These observations are caller-supplied data, not authenticated provider evidence. Adapters still
must normalize consistent snapshots and attach the last valid observation to failures. Protocol
clients, remote provider qualification, gateway accounting/admission, fallback orchestration and
hosting remain separate implementation work. No model or cloud call was made by these checks.
There is no claim of a release or completed consumer migration.
