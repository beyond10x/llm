# Provider and routing verification — 2026-09-19

The production libraries now validate independent provider/account/auth/billing/endpoint/model
bindings and resolve strict `llm.catalog/1` TOML catalogs without credential or network I/O. This
record extends the earlier core/credentials/HTTP checkpoint; it does not qualify a protocol client,
a live provider, hosting, gateway, runtime retry policy or source release.

## Executable specification

The declaration-only baseline generated **zero scenarios**. The current complete-selection ESS
suite contains **28 scenarios: 27 authored behavioral scenarios and one generated observation
adapter scenario**. All three consecutive local runs report 28 passed, 0 failed, 0 error,
0 unsupported and 0 skipped; synthesis reports 0 refusals. The generated scenario checks adapter
reachability and observation shape. The authored scenarios hold actual returned library facts to
independently written expectations.

`checks/conformance` links the real `llm-routing`, `llm-providers` and `llm-core` libraries and the
same ESS revision CI installs: `a5f1bea13294510819b266561c83be9509e6ba57` (0.26.0). No production
crate depends on ESS. The adapter accepts TOML and a versioned turn, invokes the production parsers,
`Catalog::explain` and `Catalog::resolve`, and exposes returned selection/refusal/request facts.
It does not read expected assertions or scenario names. The observation entity, local notification
and read-your-writes marker belong to this verification adapter; they do not imply a production
message bus, persistent evaluation store or inference attempt.

The suite exercises all three protocol path choices, independent subscription billing, strict
references/auth/version checks, unknown input-token refusal, inclusive context limits, conservative
implicit output limits, named capability refusals, explicit ordered fallback, preserved request
content, and opaque state refusal after same-ID repointing or cross-binding fallback. The ESS
Integer input covers nonnegative i64 bounds; the larger u64 overflow case remains a Rust regression
fixture. ESS coverage here is about these catalog observations, not every planned LLM capability.

## Gate and evidence

Run `task check` or `cargo run --locked -p b10x-llm-conformance -- check` from this checkout.
The checker regenerates the committed suite and 37 schema artifacts with ESS and refuses byte or
file-set drift. ESS's destination-specific `.ess-output` ownership ledger is not a schema artifact.
It executes the admitted suite, publishes report/2 plus detailed run/2, then re-admits the report
against the exact original suite bytes. Reports identify a SHA-256 of the runtime/checker Rust
sources, manifests and lockfile and carry the real observation time using a monotonic clock.

The committed baseline holds an answered floor of 28, a total floor of 28 and a skipped ceiling
of zero. The gate separately requires nonempty answered results, complete declared coverage and
no failures, errors or unsupported observations. There is no quarantine. An intentional removal
of an unexecutable fiction may lower total only with answered unchanged and an explained baseline
edit; none was removed here. Three consecutive runs must have identical counts.

The retained [report](routing-report.json) is paired with
[`contracts/routing/suite.json`](../../contracts/routing/suite.json). Detailed runs are written to
`target/conformance/run-{1,2,3}`; CI runs the same command and uploads `routing-conformance`.
The ordinary gate also passes 50 Rust runtime tests and 2 compile-fail documentation tests,
formatting, strict Clippy, ESS validation and AEP validation. AEP still reports the six existing
immutable review-record warnings about empty findings; this change does not rewrite those records.
CI execution in its own `ubuntu-latest` runner must be checked on the published revision separately
from these local measurements.

## Does a green run detect incorrect behavior?

Eight temporary edits to production implementations each failed a named authored scenario. Every
source file was restored byte-for-byte after its run. The observations are retained in
[routing-falsification.json](routing-falsification.json); they are deliberate counterexamples,
not failures of the restored implementation.

| Deliberate defect | Named failing scenario | Failed / passed |
| --- | --- | --- |
| Allow fallback without opt-in | `fallback-disabled` | 10 / 18 |
| Make the context boundary exclusive | `inclusive-context-boundary` | 2 / 26 |
| Admit unknown input tokens | `unknown-input-tokens-refused` | 1 / 27 |
| Select the last compatible target | `fallback-preserves-first-compatible-order` | 1 / 27 |
| Drop caller instructions | `primary-selection-preserves-request` | 9 / 19 |
| Send Messages to the Responses path | `protocol-messages-independent-of-provider` | 1 / 27 |
| Accept authenticated account without a reference | `invalid-auth-binding-refused` | 1 / 27 |
| Omit the opaque-state incompatibility reason | `repointed-endpoint-refuses-opaque-state` | 3 / 25 |

Every mutation produced zero target errors and zero unsupported observations. Each changed a
production fact the suite observes; no timeout or expected assertion was changed to produce a pass.
