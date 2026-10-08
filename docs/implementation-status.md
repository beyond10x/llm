# LLM implementation status

This record maps llm's capabilities to the repository-owned AEP stories and says what stands
behind each one. The AEP store owns lifecycle state; this page explains what those states mean for
callers. `llm-docs` generates the public status page (`website/data/status.json` and
`website/docs/status.mdx`) from the status section at the end, so a change here is a change to
the site: run `cargo run --locked -p llm-docs -- generate` after editing it.

Rules for the status section, which is the last section of this record. Each `###` heading is an
area. Each row is one capability: the owning story id (or a short id where several stories make one
capability), then `**Label.**` and what exists. A row whose text opens with `Pending` is planned;
every other row ships in the version the heading names. The label is the bold sentence, or the
first sentence when nothing is bold; the rest is the detail. Story ids stay in the first column and
out of the text, which is public.

## Evidence

The Responses overload correction in 0.3.1 recognizes `server_is_overloaded` as availability.
Loopback tests pin retry eligibility before output and finality after visible or silent output;
the default gate remains independent of live provider availability.

Every shipped row is tested in the repository gate against recorded response bytes, loopback
sockets and in-process fakes; the gate makes no paid provider call and provisions nothing. ESS
suites run the real crates three times, and `docs/verification/*-falsification.json` records the
deliberate mutations each suite catches. The [verification records](verification/) say what each
suite covers. A live probe of the Codex backend on 2026-10-04 (outside the gate) drove the 0.1.6
fix; it is not a qualification.

`connectors-secret-resolver` is deferred until Connectors supports arbitrary secret custody.
`SecretRef` does not encode a backend, so that adapter must not require editing route references
or adding a Connectors dependency to core.

## Completion requirements

The foundation is complete only when every pending row above has its acceptance evidence,
including successful API/subscription and hosting qualification. Missing qualification is a
remaining requirement, not a successful negative test. An ordinary local or CI gate makes no paid
provider call and provisions no external resource.

Consumers pin a release tag: Loom depends on llm's client crates by tag, and Harness plans to
(`harness-builds-on-llm`). The previous gateway, llmgw, stays in service until its reversible
cutover.

## Status at 0.5.0 (2026-10-08)

### Neutral turn

| Story | Capability |
| --- | --- |
| `neutral-inference` | **A neutral model turn.** `Model::turn` takes one request, a caller-owned sink and a cancellation token, and makes one attempt: bounded text and tool items, streamed events, typed failures with independent dispatch evidence, and opaque state bound to its protocol, provider, account, endpoint, model and binding revision. |
| `streamed-tool-call-name` | **Streamed tool calls carry their announced name.** The start of a streamed tool call names the tool before any argument fragment, in all three protocol projections. |
| `unattributed-opaque-state` | **Opaque state from ingress cannot be sent until bound.** Reasoning state that arrives in a client request is held unattributed, and every egress path refuses it until the caller binds it to a target of the same protocol. |
| `parity-retry-classes` | **Every failure says whether it may be retried.** `Error::retriable` and `Error::may_retry`: the transport marks 408, 429, 5xx, no response, a body failing mid-stream and a stream ending inside an event; unauthorized, refused, invalid and cancelled failures never are. |
| `runtime-contracts` | Pending: **A published contract and compatibility policy.** The versioned envelopes (`llm.turn/3`, `llm.outcome/4`, `llm.usage/2`, `llm.cost/2`, `llm.binding/1`, `llm.catalog/1`) are implemented in the tree; a released contract with its compatibility policy is not. |

### Transport

| Story | Capability |
| --- | --- |
| `http-streaming` | **Bounded HTTP and server-sent events.** One attempt per call, explicit deadlines, no redirects, no ambient proxy, bounded bodies, and partial output kept when a stream fails. |
| `parity-http-timeouts-cancel` | **Connect and idle bounds, and cancellation.** A 15-second connect timeout (`HttpClient::with_connect_timeout`), a 180-second idle bound by default, and cancellation that wins over a pending read. |
| `ess-specifications` | **Every library crate is specified in ESS.** The transport and providers joined the other domains in 0.1.3; the conformance runner runs 664 scenarios against the real crates three times. |

### Protocols

| Story | Capability |
| --- | --- |
| `chat-projection` | **Chat Completions projection and client.** `ChatClient` and an ingress codec for any compatible endpoint, including an anonymous local vLLM server; usage is always requested on a stream. |
| `messages-projection` | **Messages projection and client.** `MessagesClient` and an ingress codec; signed and redacted thinking cross as bound opaque state. |
| `parity-messages-wire` | **Messages prompt caching and unknown events.** Cache breakpoints on the standing instruction and on the conversation tail; an unknown stream event, delta or content block is kept as an opaque item with a warning. |
| `responses-projection` | **Responses projection.** Request bodies and stream decoding for the declared subset, stateless (`store: false`) and always streamed. |
| `responses-client` | **Responses client.** `ResponsesClient` implements `Model` over one Responses endpoint: the request is checked against its binding and bounded before the credential is resolved. |
| `parity-responses-live-stream` | **Live Responses streaming.** Each event reaches the caller as it arrives, and text the caller was shown stays in the turn when the terminal output omits it. |
| `parity-responses-wire` | **Responses request encoding and conversation identity.** `encode_request` gives the exact bytes the client sends; opt-in conversation identity sends `prompt_cache_key`, `session-id` and `x-client-request-id`. |
| `codex-stream` | **Responses turns against the Codex backend.** A success without a content type is read as an event stream when one was asked for, and an empty terminal output after streamed items keeps the streamed items. |

### Credentials

| Story | Capability |
| --- | --- |
| `secret-resolver` | **Injected secret resolution and coordinated renewal.** A catalog names an opaque `SecretRef`; the embedding injects the `SecretResolver`, resolution happens on every request, and renewal refreshes only the rejected generation. |
| `local-secret-adapters` | **Protected-file and keychain adapters.** Opt-in `file` (Linux, explicit absolute paths, strict ownership and mode checks), `keychain` and `native-keychain` features; read-only. |
| `parity-credential-sources` | **Environment variables and JSON pointers.** Opt-in `environment` (caller-named variables only) and `json-pointer` (an RFC 6901 pointer into a document another resolver returns). |
| `codex-auth-file` | **A Codex login, read-only.** Opt-in `codex-auth-file`: `CodexAuthFile` reads the access token of a Codex `auth.json` at an explicit absolute path on every request and refuses it once expired. |
| `parity-codex-renewal` | **Codex login renewal, opt-in.** Feature `codex-renewal`: renews inside a 15-minute margin through one non-retried request and writes only the token values back, atomically, refusing a file that changed meanwhile. |
| `codex-config-refusal` | **A misconfigured Codex login is refused.** A login file that is not a Codex login, or whose token has no readable expiry, is refused as malformed and never falls back to another account. |
| `secrets-resolver` | Pending: **Resolution through the Secrets library.** Feature `secrets`: `SecretsResolver` reads a route's reference as a name in one configured scope of the Secrets library's storage (v0.5.0), over its keychain backend or any `SecretStorage`, with refusals mapped to the existing `SecretError` codes. In the tree; not in a release yet. |

### Providers and routing

| Story | Capability |
| --- | --- |
| `provider-accounts` | **Bindings independent of protocol.** Provider, account, endpoint, model and serving declaration are separate choices; authentication and billing kind never follow from the protocol. |
| `runpod-provider-description` | Pending: **One provider description per provider.** `b10x-llm-providers`: `ProviderDescription` parses `llm.provider-description/1`, an inference URL template filled only by a 1-48 byte `[a-z0-9]` instance, its wires and authentication, and a pinned control plane (OpenAPI URL and SHA-256, HTTPS server, four operation IDs), with no I/O. Runpod's ships as `descriptions::runpod()`. Fixture evidence only; no live Runpod pod or control-plane call qualifies it. In the tree; not in a release yet. |
| `catalog-routing` | **TOML catalogs, selection and explanation.** Strict `llm.catalog/1`, a deterministic configuration digest, ordered opt-in selection, capability and input-token admission, and an explanation that resolves no secret. |
| `ordered-fallback` | **Ordered fallback.** `Catalog::run_turn` tries a route's declared targets in order and stops on visible output, an ineligible failure, an ambiguous dispatch, the attempt bound, the deadline, cancellation or the caller's limit. |
| `same-target-retry` | **Same-target retry before visible output.** `RetryPolicy`, on by default: four attempts per target with 1, 2 and 4 second waits, server delays honoured up to 30 seconds, a `turn-retried` warning before each wait, then fallback. |
| `catalog-model-port` | Pending: **One function builds the port a serving model declares.** `b10x-llm-models`: `port` builds the Chat Completions, Responses or Messages client a catalog binding's protocol names, with the caller's resolver for its account, and refuses a credentialed account without one as `unauthorized` before any I/O; `CatalogModels` builds every serving model's port and is the `Models` ordered fallback takes. In the tree; not in a release yet. |
| `openai-access` | Pending: **OpenAI access qualified.** API and caller-managed subscription routes with recorded live evidence. A live probe of the Codex backend found the two incompatibilities 0.1.6 fixed; no route is qualified. |
| `anthropic-access` | Pending: **Anthropic access qualified.** API and caller-managed subscription routes with recorded live evidence. The subscription presentation is built and fixture-checked: a `subscription-oauth` account resolves its token through its secret reference and Messages sends it as a bearer with the OAuth beta header and the client preamble; no live turn is recorded. |

### Helpers for agent loops

| Story | Capability |
| --- | --- |
| `call-tool-helper` | **One forced tool call.** `b10x-llm-tool-call`: `call_tool` forces one named tool on any `Model` and returns that call's JSON arguments or a typed `ModelError`; `codex_model` binds a Responses model to the operator's Codex login. |
| `parity-blocking-adapters` | **A blocking adapter for a synchronous loop.** `b10x-llm-blocking`: `BlockingModel` runs one turn on the calling thread, hands events to a `BlockingSink` as they arrive, takes cancellation from any thread, and forks for concurrent turns. |
| `harness-builds-on-llm` | Pending: **Harness builds on llm.** Harness replaces its own model wire crates with llm's; the parity matrix maps every capability they need. |

### Accounting

| Story | Capability |
| --- | --- |
| `usage-pricing` | **Usage pricing.** Versioned `llm.prices/1` books, exact decimal amounts, cache and compute pricing, six separate bases and unknown quantities kept unknown. |
| `spending-limits` | **Durable spending limits.** Opt-in `sqlite`: reservations committed before dispatch, concurrent callers serialized, uncertain charges kept across restart, and compute stop obligations. |

### Serving

| Story | Capability |
| --- | --- |
| `serving-extraction` | **The gateway and hosting crates live in llm-gateway.** `b10x-llm-gateway`, `b10x-llm-provision`, `b10x-llm-runpod` and `b10x-llm-modal` moved with their tests and their ESS domains to their own repository; its status covers the gateway, the hosting contract, the Runpod adapter and their pending work, and llm keeps the client side. |
| `llmgw-retirement` | Pending: **Deployments move from llmgw.** A reversible cutover from the previous gateway to llm-gateway. |
| `operator-cli` | Pending: **An operator command line.** Validate, inspect and run one configuration; `b10x-llm-cli` exports nothing yet. |

### Release

| Story | Capability |
| --- | --- |
| `foundation-qualified` | Pending: **A qualified release.** One exact release tied to its required checks, artifacts and live provider and hosting qualification. |
