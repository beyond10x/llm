# LLM implementation status

The goal is the full agreed foundation and subsequent consumer adoption, not a compiling
workspace. This record maps implementation evidence to the repository-owned stories. The AEP
store owns lifecycle state; this page explains what those states mean for callers.

## Foundation checkpoint — 2026-09-19

Five libraries now implement the shared boundary. The neutral core supports asynchronous model
turns, text/tools, bounded streaming, cancellation, optional usage, typed failures, and opaque
continuation state bound to its exact protocol/provider/account/endpoint/model/binding revision. Credentials are
injected and resolved on each request; concurrent renewal is coordinated without owning a login
or persistent credential store. The HTTP transport streams bounded SSE with explicit deadlines,
no redirects, no automatic retries, and failure after partial output preserved.

Provider bindings validate independent protocol, provider, auth and billing choices, including
anonymous arbitrary endpoints. Routing validates strict TOML catalogs, preserves ordered opt-in
selection, checks capabilities and conservative input-token bounds, and explains safe refusal
reasons without resolving secrets. Runtime fallback after an attempted request is still pending.

The [initial verification record](verification/core-foundation.md) records the first foundation
checkpoint. [Routing verification](verification/routing-conformance.md) adds executable ESS
behavior, generated-schema drift checks and deliberate mutation evidence. This is fixture evidence
for these libraries, not a usable remote model client or release qualification. Owning stories remain active
while the versioned configuration surface and release prerequisites are completed.

| Required story | Implementation and remaining work |
| --- | --- |
| `runtime-contracts` | Unreleased turn/outcome revision 2, binding/catalog v1 and ESS verification implemented; release/common Gates setup remains. |
| `neutral-inference` | Public async port, bounded data, tool round trip, cancellation and embedding example implemented and tested. |
| `http-streaming` | Bounded single-attempt HTTP/SSE, terminal truth, cancellation, deadline and retry-hint fixtures pass. |
| `secret-resolver` | Injected arbitrary secret references, redacted/zeroized material and coordinated caller-owned renewal implemented and tested. |
| `provider-accounts` | Validated bindings, arbitrary endpoint URLs and selected-reference request-time auth implemented; live access qualification is separate. |
| `local-secret-adapters` | Pending: explicit optional file and OS keychain adapters. |
| `responses-projection` | Pending: Responses request, output and streaming projections. |
| `messages-projection` | Pending: Messages request, output and streaming projections. |
| `chat-projection` | Pending: Chat Completions projections and arbitrary compatible endpoints. |
| `openai-access` | Pending: API and caller-managed subscription presentation and successful qualification. |
| `anthropic-access` | Pending: API and caller-managed subscription presentation and successful qualification. |
| `catalog-routing` | Strict versioned TOML, deterministic identity, safe explanation, ordered selection and capability admission implemented and tested. |
| `ordered-fallback` | Pending: explicit ordered alternatives, attempt accounting and refusal after exposed output or uncertain acceptance. |
| `usage-pricing` | Pending: versioned price inputs and attributable known/estimated/unknown charges. |
| `spending-limits` | Pending: model budget ownership and implement reservations, concurrency, restart and uncertain-charge policy. |
| `hosting-contract` | Pending: model owned-resource lifecycle, leases, reconciliation and cleanup before adapter implementation. |
| `runpod-hosting` | Pending: port and qualify Runpod vLLM deployment mechanics against the hosting contract. |
| `modal-hosting` | Pending: implement and qualify supported Modal lifecycle operations. |
| `gateway-auth` | Pending: single-owner authenticated gateway. |
| `gateway-translation` | Pending: three ingress protocols, explicit supported subset and streaming/tool/cancellation semantics. |
| `operator-cli` | Pending: validate, inspect and run one configuration; inspection must not resolve secrets or provision resources. |
| `foundation-qualified` | Pending: exact release, required checks/artifacts and all required implementation/qualification evidence. |

The twenty-third story, `connectors-secret-resolver`, is explicitly deferred until Connectors
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
`story:chat-projection`; no driver run has launched while operator USD terms remain pending.
Preserve independent auth, billing and protocol choices;
an anonymous vLLM binding must be explicit. Continue recording acceptance evidence in each owning
story. Model the remaining budget and hosting runtime semantics before implementing their stores.
