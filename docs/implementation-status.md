# LLM implementation status

The goal is the full agreed foundation and subsequent consumer adoption, not a compiling
workspace. This record maps implementation evidence to the repository-owned stories. The AEP
store owns lifecycle state; this page explains what those states mean for callers.

## Foundation checkpoint — 2026-09-19

Six libraries now implement the shared boundary. The neutral core supports asynchronous model
turns, text/tools, bounded streaming, cancellation, bound success/failure observations with usage finality, typed failures, and opaque
continuation state bound to its exact protocol/provider/account/endpoint/model/binding revision. Credentials are
injected and resolved on each request; concurrent renewal is coordinated without owning a login
or persistent credential store. The HTTP transport streams bounded SSE with explicit deadlines,
no redirects, a single attempt per call with a retry class on every refusal, and failure after
partial output preserved. Routing retries a retriable failure on the same target before any
output is visible (Harness policy by default), then falls back.

Optional local secret adapters now read explicitly mapped protected files on Linux or an injected
keychain store. Native constructors select Linux Secret Service, macOS Keychain or Windows
Credential Manager. The `codex-auth-file` adapter reads a Codex login's access token from an
explicit absolute `auth.json` path, read-only and refused once expired; the opt-in `codex-renewal`
feature renews it through its token endpoint and writes it back atomically and byte-preserving. [Adapter documentation](local-secrets.md) records the platform and trust
boundaries; tests use disposable files and mock stores, never existing user credentials.

Provider bindings validate independent protocol, provider, auth and billing choices, including
anonymous arbitrary endpoints. Routing validates strict TOML catalogs, preserves ordered opt-in
selection, checks capabilities and conservative input-token bounds, and explains safe refusal
reasons without resolving secrets. Runtime fallback after an attempted request is still pending.

Pricing now validates explicit versioned JSON/TOML rates and prices attributed token/cache and
resource-millisecond observations. Exact decimal arithmetic preserves unknown quantities and
separates metered/compute estimates, reference usage valuations and recorded charges. Failed and
uncertain attempts are retained. [Pricing](pricing.md) documents the caller-observation contract;
end-to-end provider attribution remains separate work.

The optional SQLite budget ledger now persists reservations before dispatch, serializes concurrent
callers, excludes another process owner, retains uncertain charges after restart and exposes compute
shutdown obligations. [Budgets](budgets.md) defines the declared-estimate policy and trusted-storage
boundary. Gateway/fallback/hosting integration and actual cloud shutdown remain separate work.

The [initial verification record](verification/core-foundation.md) records the first foundation
checkpoint. [Routing verification](verification/routing-conformance.md) adds executable ESS
behavior, generated-schema drift checks and deliberate mutation evidence. This is fixture evidence
for these libraries, not a usable remote model client or release qualification. Owning stories remain active
while the versioned configuration surface and release prerequisites are completed.

| Required story | Implementation and remaining work |
| --- | --- |
| `runtime-contracts` | Unreleased turn v2/outcome v3, usage/cost v2, binding/catalog v1 and ESS verification implemented; release/common Gates setup remains. |
| `neutral-inference` | Public async port, bounded data, tool round trip, cancellation and embedding example implemented and tested. |
| `http-streaming` | Bounded single-attempt HTTP/SSE, terminal truth, cancellation, deadline, retry-class and retry-hint fixtures pass; same-target retry before visible output is in routing. |
| `secret-resolver` | Injected arbitrary secret references, redacted/zeroized material and coordinated caller-owned renewal implemented and tested. |
| `provider-accounts` | Validated bindings, arbitrary endpoint URLs and selected-reference request-time auth implemented; live access qualification is separate. |
| `local-secret-adapters` | Explicit file/keychain adapters implemented; Linux file protections, exact mock-store lookup, rotation and fixed errors tested. Native OS-service availability is not established by mock tests or compilation. |
| `responses-projection` | Implemented (story implemented in the AEP store, shipped in 0.1.0): Responses request, output and streaming projections. |
| `responses-client` | Implemented, unreleased: `ResponsesClient`, a single-attempt `Model` over one bound Responses endpoint, tested against local sockets only. |
| `messages-projection` | Implemented (story implemented in the AEP store, shipped in 0.1.0): Messages request, output and streaming projections. |
| `chat-projection` | Implemented (story implemented in the AEP store, shipped in 0.1.0): Chat Completions projections and arbitrary compatible endpoints. |
| `openai-access` | Pending: API and caller-managed subscription presentation and successful qualification. |
| `anthropic-access` | Pending: API and caller-managed subscription presentation and successful qualification. |
| `catalog-routing` | Strict versioned TOML, deterministic identity, safe explanation, ordered selection and capability admission implemented and tested. |
| `ordered-fallback` | Implemented (story implemented in the AEP store, shipped in 0.1.0): explicit ordered alternatives, attempt accounting and refusal after exposed output or uncertain acceptance. |
| `usage-pricing` | Versioned price books, exact amounts, cache/compute pricing, attributed unknowns, failed attempts and separate recorded/subscription charges implemented with fixtures and ESS; live provider observations remain unqualified. |
| `spending-limits` | Single-owner policy, pure engine, SQLite journal, concurrent admission, one-shot starts, restart uncertainty, overrun retention and compute stop obligations implemented with real local storage fixtures and ESS. Effectful consumers still need to use the ledger. |
| `hosting-contract` | Implemented (story implemented in the AEP store, shipped in 0.1.0): model owned-resource lifecycle, leases, reconciliation and cleanup before adapter implementation. |
| `runpod-hosting` | Implemented (story implemented in the AEP store, shipped in 0.1.0): port and qualify Runpod vLLM deployment mechanics against the hosting contract. |
| `modal-hosting` | Pending: implement and qualify supported Modal lifecycle operations. |
| `gateway-auth` | Implemented (story implemented in the AEP store, shipped in 0.1.0): single-owner authenticated gateway. |
| `gateway-translation` | Pending: three ingress protocols, explicit supported subset and streaming/tool/cancellation semantics. |
| `operator-cli` | Pending: validate, inspect and run one configuration; inspection must not resolve secrets or provision resources. |
| `foundation-qualified` | Pending: exact release, required checks/artifacts and all required implementation/qualification evidence. |

One further story, `connectors-secret-resolver`, is explicitly deferred until Connectors
supports arbitrary secret custody. `SecretRef` does not encode a backend, so this adapter must not
require editing route references or adding a Connectors dependency to core.

## Completion requirements

The foundation is complete only when every required story above has its acceptance evidence,
including successful API/subscription and hosting qualification. Missing qualification is a
remaining requirement, not a successful negative test. An ordinary local or CI gate makes no paid
provider call and provisions no external resource.

Harness and Metaharness migration plans are held in their own AEP stores; the llmgw store holds
reversible retirement. None has migrated in this checkpoint. Consumer adoption uses a released
or explicitly qualified exact revision, and existing llmgw operation continues until cutover.
Atlas holds the catalog/release-unit declaration. Planning publication in Harness and Atlas has
separate recorded authorship-policy blockers; those blockers do not supply implementation evidence
or authorize changing source-publication controls.

## Next implementation step

Implement protocol projections using the shared transport. The draft governed task names
`story:chat-projection`; no driver run has launched while operator map selection and USD terms remain pending.
Preserve independent auth, billing and protocol choices;
an anonymous vLLM binding must be explicit. Continue recording acceptance evidence in each owning
story. Model the remaining hosting runtime semantics before implementing its controllers; consume
the budget ledger's permits and shutdown obligations in the later effectful integrations.
