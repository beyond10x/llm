# Local secret adapter verification — 2026-09-19

Historical checkpoint at `21be0093813283038db6aa28ac4b940b78faa7e4`. The counts and report below
belong to that exact revision. [Pricing verification](pricing.md) retains all 52 checks and extends
the current combined suite. Linux, macOS and Windows [CI passed for this checkpoint](https://github.com/beyond10x/llm/actions/runs/35434173402), and its downloaded suite, schemas and reports were verified.

The optional file and keychain adapters now resolve the injected `SecretRef` contract. The file
backend enforces Linux descriptor, ownership, mode, link, size and metadata checks. Keychain uses
an explicitly injected store and exact service/entry lookup; an optional constructor selects the
native OS implementation. Both reread each request and use a redacted content identity to expose
rotation without owning credential renewal. See [usage and limits](../local-secrets.md).

## ESS behavior and counted evidence

The previous routing checkpoint had 28 passed scenarios. The combined complete-selection suite
now has **52 scenarios: 49 authored and 3 generated observation checks**. This adds 22 authored
secret behaviors and two generated adapter checks; none of the 28 previous routing checks was
removed. All three consecutive restored-source local runs report **52 passed, 0 failed, 0 error,
0 unsupported, 0 skipped**. Synthesis reports zero refusals. These counts come from report/2,
not a runner exit code. No skips became failures and no fictional branch was removed.

`contracts/ess-inputs.yaml` explicitly selects the routing and secret scenario files.
The [original suite](https://github.com/beyond10x/llm/blob/21be0093813283038db6aa28ac4b940b78faa7e4/contracts/suite.json) is paired with the retained
[report](local-secrets-report.json). The same pinned ESS 0.26.0 runner admits original suite bytes,
executes the actual libraries and re-admits the persisted report. The gate checks an answered floor
of 52, total floor of 52, skipped ceiling zero, complete declared coverage and zero failures,
errors or unsupported observations. It compares three consecutive counts and regenerates the
suite plus 47 schemas to reject projection drift. There is no quarantine.

The `llm.secrets` domain describes this test adapter's observations, not a production credential
database or event service. Commands create disposable real file fixtures or explicitly injected
keyring-core mock entries. Views contain facts from returned bytes, versions and typed errors;
the target has no access to scenario names or expected assertions. The 1 MiB oversize witness is
an independent literal contract boundary, so changing the implementation constant cannot move
the witness with it. Secret debug output must match the approved redacted forms, including when
an incorrect implementation prints bytes as integers instead of text.

## Falsification

Nine deliberate production mutations each fail a named authored scenario. All modified source
files were restored byte-for-byte. [Exact mutations and reports](local-secrets-falsification.json)
retain source hashes, counts and failing names.

| Deliberate defect | Named failure | Failed / error / passed |
| --- | --- | --- |
| Admit publicly readable files | `file-public-readable-refused` | 1 / 0 / 51 |
| Admit writable parents | `file-writable-parent-refused` | 1 / 0 / 51 |
| Follow symlinks | `file-symlink-refused` | 2 / 0 / 50 |
| Admit hard links | `file-hardlink-refused` | 1 / 0 / 51 |
| Resolve another keychain entry | `keychain-exact-store-and-entry` | 7 / 1 / 44 |
| Reuse one content version | `file-rereads-rotation` | 2 / 0 / 50 |
| Double the secret byte limit | `keychain-size-bound` | 2 / 0 / 50 |
| Classify backend failures as missing | `keychain-backend-error-redacted` | 1 / 0 / 51 |
| Expose raw bytes through Debug | `keychain-exact-store-and-entry` | 8 / 0 / 44 |

The wrong-entry mutation also produces one target error in the generated keychain probe: bypassing
the selected entry leaves its injected one-shot fault pending, and the fixture's later rotation
encounters it. That error is retained, not counted as an assertion failure or coverage improvement.
Seven authored checks independently fail, including the named lookup assertion. All mutations
have zero unsupported/skipped observations. The restored implementation has zero target errors.

## Other gates and qualification limits

`task check` passes 56 unique runtime tests and two compile-fail documentation tests, default and
all-feature credential builds, formatting, strict Clippy, ESS and AEP validation. Rust tests also
cover raw binary bytes, restoring a prior value, path traversal, execution/special mode bits,
FIFO refusal without a writer and cancellation retaining the blocking-operation permit.
Six existing immutable review records still carry the recorded missing-findings warnings.

CI runs the same gate commands on `ubuntu-latest`; dedicated macOS and Windows jobs compile native
backends and run mock-store/platform-refusal tests. Check the exact published revision's CI result
before claiming those remote jobs passed. Artifacts are uploaded as `foundation-conformance`.
No live user keychain, paid model or hosting resource is read, called or provisioned by these tests.
Native-service availability/unlock, foreign-owner filesystem fixtures and adversarial metadata
races are not qualified by this suite. The file adapter deliberately refuses non-Linux platforms.
The overall foundation, source release and consumer migrations remain incomplete.
