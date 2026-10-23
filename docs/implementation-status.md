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

The Responses overload correction in 0.3.1 recognizes `server_is_overloaded` as availability;
0.8.0 retries Responses `server_error` and `rate_limit_exceeded` and Messages `overloaded_error`
and `api_error` by the same rule (`story:retry-classes-harness-retried`); the next release makes a
quota 429 final (`story:quota-429-is-final`) and honours `x-should-retry: false`
(`story:relayed-failures-keep-their-retry-decision`).
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
including successful API and subscription qualification. Missing qualification is a
remaining requirement, not a successful negative test. An ordinary local or CI gate makes no paid
provider call and provisions no external resource.

Consumers pin a release tag and move only on their own change. Loom, inference and llm-gateway
depend on llm's crates by tag (Atlas ADRs 0098 and 0100 for the first two). Harness's cutover
onto 0.7.0 is decided (Atlas ADR 0097) and is Harness's own work; `docs/harness-parity.md` is
current to 0.7.0 for it (`harness-cutover-on-llm-0-7`). Serving, hosting and their qualification
are llm-gateway's ([GitHub](https://github.com/beyond10x/llm-gateway)).

## Status at 0.13.0 (2026-10-23)

### Neutral turn

| Story | Capability |
| --- | --- |
| `neutral-inference` | **A neutral model turn.** `Model::turn` takes one request, a caller-owned sink and a cancellation token, and makes one attempt: bounded text and tool items, streamed events, typed failures with independent dispatch evidence, and opaque state bound to its protocol, provider, account, endpoint, model and binding revision. |
| `streamed-tool-call-name` | **Streamed tool calls carry their announced name.** The start of a streamed tool call names the tool before any argument fragment, in all three protocol projections. |
| `unattributed-opaque-state` | **Opaque state from ingress cannot be sent until bound.** Reasoning state that arrives in a client request is held unattributed, and every egress path refuses it until the caller binds it to a target of the same protocol. |
| `parity-retry-classes` | **Every failure says whether it may be retried.** `Error::retriable` and `Error::may_retry`: the transport marks 408, 429 (a 429 a client's quota rule reads as an exhausted quota excepted), 5xx, no response, a body failing mid-stream and a stream ending inside an event; unauthorized, refused, invalid and cancelled failures never are. An error status carrying `x-should-retry: false` is final whatever its status. |
| `neutral-turn-controls` | **Thinking, parallel tool calls and advisory hints in the neutral turn.** `TurnRequest.controls` carries a thinking mode, the parallel-tool-call choice and advisory hints (prompt cache, cache key, verbosity, reasoning summary, `anthropic-beta` names, unforwarded field names); `wire_effort` is the one reasoning table (`max` reaches Chat Completions and Responses as `xhigh`), and a turn with controls is `llm.turn/4`. |
| `runtime-contracts` | **A written contract and compatibility policy.** `docs/contract-v1.md` states turn ownership, validation, accounting, pricing, credentials and compatibility; the versioned envelopes (`llm.turn/3` and `/4`, `llm.outcome/4`, `llm.usage/2`, `llm.cost/2`, `llm.binding/1`, `llm.catalog/1`) refuse unknown versions and fields. |
| `capabilities-declare-streaming` | **A model declares whether its output streams and whether input can reach a running turn.** `Capabilities` carries `output_streaming` and `input_during_output`; every client here declares `true` and `false`, a document written before them keeps its binding revision and configuration digest, and a binding, catalog or route claiming input during output or unstreamed output is refused. |

### Transport

| Story | Capability |
| --- | --- |
| `http-streaming` | **Bounded HTTP and server-sent events.** One attempt per call, explicit deadlines, no redirects, no ambient proxy, bounded bodies, and partial output kept when a stream fails. |
| `parity-http-timeouts-cancel` | **Connect and idle bounds, and cancellation.** A 15-second connect timeout (`HttpClient::with_connect_timeout`), a 180-second idle bound by default, and cancellation that wins over a pending read. |
| `ess-specifications` | **Every library crate is specified in ESS.** The transport and providers joined the other domains in 0.1.3; the conformance runner answers at least 937 scenarios against the real crates three times and skips none. |

### Protocols

| Story | Capability |
| --- | --- |
| `chat-projection` | **Chat Completions projection and client.** `ChatClient` and an ingress codec for any compatible endpoint, including an anonymous local vLLM server or llama.cpp `llama-server`; usage is always requested on a stream, and llama-server's `timings` counters are read beside it. |
| `messages-projection` | **Messages projection and client.** `MessagesClient` and an ingress codec; signed and redacted thinking cross as bound opaque state. |
| `parity-messages-wire` | **Messages prompt caching and unknown events.** Cache breakpoints on the standing instruction and on the conversation tail; an unknown stream event, delta or content block is kept as an opaque item with a warning. |
| `responses-projection` | **Responses projection.** Request bodies and stream decoding for the declared subset, stateless (`store: false`) and always streamed. |
| `responses-client` | **Responses client.** `ResponsesClient` implements `Model` over one Responses endpoint: the request is checked against its binding and bounded before the credential is resolved. |
| `parity-responses-live-stream` | **Live Responses streaming.** Each event reaches the caller as it arrives, and the turn carries the text the caller was shown: kept when the terminal output omits it, and kept over a server's own message whose text differs, with a `shown-text-kept` warning; text it cannot tie to one message overwrites none, with a `shown-text-unpaired` warning. |
| `text-deltas-are-final` | **Streamed text is final.** No event retracts or replaces one already emitted; a successful turn's assistant text is exactly the text deltas shown for it, on Responses, Messages and Chat Completions, and Responses says when it is not. |
| `parity-responses-wire` | **Responses request encoding and conversation identity.** `encode_request` gives the exact bytes the client sends; opt-in conversation identity sends `prompt_cache_key`, `session-id` and `x-client-request-id`. |
| `responses-ingress-encoder` | **A server answers a Responses caller.** `encode_ingress_response` and `IngressStream` encode neutral events and an outcome as the Responses event payloads and response object this crate's decoder reads back as the same turn; partial usage and the error's message are never relayed. |
| `messages-ingress-encoder` | **A server answers a Messages caller.** `encode_ingress_message` and `IngressStream` encode the Messages event stream and message, each block, reason and usage report checked against this crate's decoder; `without_reasoning_relay` for an upstream that cannot sign reasoning. |
| `messages-ingress-client-fields` | **Claude Code's first turn under written Messages rules.** Ingress carries `thinking`, a `system` of text blocks, `cache_control` as the prompt-cache hint, effort `max` and `disable_parallel_tool_use`; `metadata` is reported, never sent; everything else is refused by its path. Checked against llm-gateway's authored fixture, not a captured session. |
| `responses-ingress-client-fields` | **Codex's requests under written Responses rules.** Ingress carries `instructions`, `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity` and `reasoning.summary`, reads a plain-string tool result, reports item ids and `client_metadata` as unsent, and refuses every unknown key at every level by its path and hosted tools by type. Checked against requests captured from Codex 0.160.0 at a loopback recorder; no provider was called. |
| `chat-ingress-closed-levels` | **Chat ingress closed at every level.** Every unknown key below the top is refused by its path, `strict` schemas are refused, and `parallel_tool_calls`, `prompt_cache_key` and `verbosity` are carried both ways. |
| `tool-result-rejected-or-failed` | **A failed tool result reads back as failed, and a rejected call has one encoding.** Chat ingress reads its own failure object as `failed: true`; a call rejected before it ran is sent as `failed: true` with the caller's reason in `output`, on every wire, because none carries a rejected state. |
| `client-turns-cross-wire` | **A Claude Code and a Codex first turn cross wires.** Each decodes on its own wire and projects onto the other two with every field under a written rule; each projection lists the advisory hints it omits, and each client warns `advisory-omitted` after a successful turn. |
| `codex-stream` | **Responses turns against the Codex backend.** A success without a content type is read as an event stream when one was asked for, and an empty terminal output after streamed items keeps the streamed items. |
| `retry-classes-harness-retried` | **The provider failures Harness retried are retriable before output.** A Responses `server_error` or `rate_limit_exceeded` and a Messages `overloaded_error` or `api_error` stream error may be retried before any output and are final after it, the rule Responses `server_is_overloaded` already followed; an exhausted quota stays final inside a stream and, since `story:quota-429-is-final`, as an HTTP 429 before it (Responses by `error.code`, or by `error.type` for `insufficient_quota` and the Codex backend's `usage_limit_reached` and `usage_not_included`; Messages a `rate_limit_error` whose `error.details.error_code` is `enforced_spend_limit_reached`). A failure an llm gateway relays inside a stream with nothing streamed first still reads back retriable; a failure before output, the gateway's HTTP 502, reads final when it carries `x-should-retry: false`, which llm-gateway has to send. |

### Amazon Bedrock

| Story | Capability |
| --- | --- |
| `bedrock-signing` | **Bedrock requests are signed with Signature Version 4.** `llm-bedrock` signs each request with a signer written on `ring`, checked against the AWS test suite vendored from aws-c-auth (header signing only); keys come from `StaticCredentials` through an injected resolver or from `ContainerCredentials` at the container credentials endpoint. No AWS SDK crate and no `aws-lc` is a dependency. EKS Pod Identity is described from AWS documentation and unverified. |
| `bedrock-event-stream` | **AWS event-stream frames decode, bounded.** `llm_bedrock::eventstream` decodes frames incrementally across reads, at most 16 MiB per frame; checked against the smithy-rs vectors and recorded Bedrock bodies. |
| `bedrock-converse` | **A turn runs on Bedrock Converse.** `BedrockClient` with `Api::Converse` sends `/converse-stream` or `/converse` and reads text, tool calls, thinking and usage; the body carries no `cachePoint`, so an absent cache counter is a known 0 and the input total is known whenever `inputTokens` is. Fixture evidence only; no live Bedrock call was made. |
| `bedrock-anthropic-invoke` | **A turn runs on Anthropic models through Bedrock InvokeModel.** `Api::AnthropicInvoke` sends the Messages body with `anthropic_version: bedrock-2023-05-31` and reads the stream with the Messages decoder; invocation metrics fill only counters Messages usage left unreported. Fixture evidence only. |
| `bedrock-specification` | **Bedrock behaviour is specified in ESS.** The `llm.bedrock` domain and 43 authored scenarios cover signing, frame decoding, container credentials and whole turns against loopback servers; the conformance runner answers at least 937 scenarios and skips none. |
| `bedrock-followups` | **Bedrock pre-send check, non-blocking credential refresh and Anthropic betas.** `llm_bedrock::check_request` and `encode_request` refuse before I/O exactly what a turn refuses, on both wires; the client uses the same function. `ContainerCredentials` answers kept, unexpired credentials at once inside the refresh window, runs at most one fetch, on its own task, and makes callers wait only once nothing unexpired is kept; a failed refresh keeps the unexpired credentials. The `anthropic-beta` names a Messages client opts into (`Hints::anthropic_beta`, read by `decode_request_with_headers`) reach the Messages route as one header and InvokeModel as the body's `anthropic_beta`; Converse, Chat and Responses report them omitted. Fixture evidence only. |
| `bedrock-credential-process` | **Bedrock keys from a credential command.** `llm.routes/1` takes `auth = { kind = "aws-process", command = [...] }` on a Bedrock endpoint, in YAML and TOML. `llm_bedrock::ProcessCredentials` (feature `process`) runs the command without a shell, stdin empty, stderr inherited, stdout read up to 64 KiB, killed after 60 seconds, and reads the AWS `credential_process` answer; it runs at the first turn and again once inside the five-minute refresh window, one run at a time, and the keys stay in memory. A failed run or unusable answer is `unauthorized`, naming the endpoint and the program, carrying none of the output, sending nothing. `llm-models` builds the route only with feature `process` and the caller's `RouteResolvers::allow_credential_process`; the `llm` command allows it, refuses on Unix a routes file declaring one that is not a regular file owned by you or root, group- or world-writable, or reached through a directory others can write, and `llm model list` never runs it. Once the command exits, its output is taken within 250 ms, so a process it leaves holding stdout does not hold the answer. Fixture evidence only; the AWS CLI was not run. |

### Credentials

| Story | Capability |
| --- | --- |
| `secret-resolver` | **Injected secret resolution and coordinated renewal.** A catalog names an opaque `SecretRef`; the embedding injects the `SecretResolver`, resolution happens on every request, and renewal refreshes only the rejected generation. |
| `local-secret-adapters` | **Protected-file and keychain adapters.** Opt-in `file` (Linux, explicit absolute paths, strict ownership and mode checks; `FileResolver::mounted` for a platform-mounted file such as a Kubernetes projected volume), `keychain` and `native-keychain` features; read-only. |
| `parity-credential-sources` | **Environment variables and JSON pointers.** Opt-in `environment` (caller-named variables only) and `json-pointer` (an RFC 6901 pointer into a document another resolver returns). |
| `codex-auth-file` | **A Codex login, read-only.** Opt-in `codex-auth-file`: `CodexAuthFile` reads the access token of a Codex `auth.json` at an explicit absolute path on every request and refuses it once expired. |
| `parity-codex-renewal` | **Codex login renewal, opt-in.** Feature `codex-renewal`: renews inside a 15-minute margin through one non-retried request and writes only the token values back, atomically, refusing a file that changed meanwhile. |
| `codex-config-refusal` | **A misconfigured Codex login is refused.** A login file that is not a Codex login, or whose token has no readable expiry, is refused as malformed and never falls back to another account. |
| `secrets-resolver` | **Resolution through the Secrets library.** Feature `secrets`: `SecretsResolver` reads a route's reference as a name in one configured scope of the Secrets library's storage (v0.5.0), over its keychain backend or any `SecretStorage`, with refusals mapped to the existing `SecretError` codes. |

### Providers and routing

| Story | Capability |
| --- | --- |
| `provider-accounts` | **Bindings independent of protocol.** Provider, account, endpoint, model and serving declaration are separate choices; authentication and billing kind never follow from the protocol. |
| `runpod-provider-description` | **One provider description per provider.** `llm-providers`: `ProviderDescription` parses `llm.provider-description/1`, an inference URL template filled only by a 1-48 byte `[a-z0-9]` instance, its wires and authentication, and a pinned control plane (OpenAPI URL and SHA-256, HTTPS server, four operation IDs), with no I/O. Runpod's ships as `descriptions::runpod()`. Fixture evidence only; no live Runpod pod or control-plane call qualifies it. |
| `catalog-routing` | **TOML catalogs, selection and explanation.** Strict `llm.catalog/1`, a deterministic configuration digest, ordered opt-in selection, capability and input-token admission, and an explanation that resolves no secret. |
| `ordered-fallback` | **Ordered fallback.** `Catalog::run_turn` tries a route's declared targets in order and stops on visible output, an ineligible failure, an ambiguous dispatch, the attempt bound, the deadline, cancellation or the caller's limit. |
| `same-target-retry` | **Same-target retry before visible output.** `RetryPolicy`, on by default: four attempts per target with 1, 2 and 4 second waits, server delays honoured up to 30 seconds, a `turn-retried` warning before each wait, then fallback. |
| `routes-document` | **One routes document for every consumer.** `llm-models`: `llm_models::routes::Routes` reads `llm.routes/1` (YAML endpoints with base URL, protocol and a secret reference; routes as `<endpoint>/<model-id>`, or direct names), expands it into an `llm.catalog/1` catalog and builds `RouteResolvers` from `env:`, `file:` and `keychain:` references (features `environment`, `file`, `keychain`), refusing an unset variable or a missing file by the endpoint's reference before any request. Fixture evidence only. |
| `llm-command` | **The `llm` command.** `llm-cli`: `llm model list` prints the routes an `llm.toml` (or YAML) routes file declares, with aliases, host and price, reading no secret; `llm infer` streams one turn on a route, alias or `<endpoint>/<model-id>` and closes with its usage and cost on stderr, unknown counters printed `unknown`. `llm.routes/1` gains TOML, aliases, per-route prices and Bedrock endpoints (`aws-keys` or `aws-container`), whose routes `Routes::port` serves with `BedrockClient`. It reads `keychain:` references from the platform keychain, opened only for a route that needs it; an endpoint whose source cannot be built refuses only its own routes. A closed stdout stops the turn with exit 0; a second Ctrl-C drops the turn with 130. The container token file may be a Kubernetes projected volume (`FileResolver::mounted`; layout from AWS and Kubernetes documentation, unverified). Fixture evidence only. |
| `catalog-model-port` | **One function builds the port a serving model declares.** `llm-models`: `port` builds the Chat Completions, Responses or Messages client a catalog binding's protocol names, with the caller's resolver for its account, and refuses a credentialed account without one as `unauthorized` before any I/O; `CatalogModels` builds every serving model's port and is the `Models` ordered fallback takes. |
| `openai-access` | Pending: **OpenAI access qualified.** API and caller-managed subscription routes with recorded live evidence. A live probe of the Codex backend found the two incompatibilities 0.1.6 fixed; no route is qualified. |
| `anthropic-access` | Pending: **Anthropic access qualified.** API and caller-managed subscription routes with recorded live evidence. The subscription presentation is built and fixture-checked: a `subscription-oauth` account resolves its token through its secret reference and Messages sends it as a bearer with the OAuth beta header and the client preamble; no live turn is recorded. |

### Helpers for agent loops

| Story | Capability |
| --- | --- |
| `call-tool-helper` | **One forced tool call.** `llm-tool-call`: `call_tool` forces one named tool on any `Model` and returns that call's JSON arguments or a typed `ModelError`; `codex_model` binds a Responses model to the operator's Codex login. |
| `parity-blocking-adapters` | **A blocking adapter for a synchronous loop.** `llm-blocking`: `BlockingModel` runs one turn on the calling thread, hands events to a `BlockingSink` as they arrive, takes cancellation from any thread, and forks for concurrent turns. |
| `harness-builds-on-llm` | **Harness builds on llm.** Atlas ADR 0097 decided the cutover: Harness dropped its own model wire and credential crates and builds on llm 0.8.0 since Harness commit `97ff3d7f` on its `main` (2026-10-20); its spike had built and passed its tests on llm 0.1.7. `docs/harness-parity.md` cites every llm cell against 0.7.0 and lists the llm API changes the bridge meets; one needs an edit (`TurnRequest.controls`). Rows R28 and M27 are partial at 0.7.0, which marks fewer provider failures retriable than Harness's wires did; 0.8.0 retries those classes before any output. |

### Accounting

| Story | Capability |
| --- | --- |
| `usage-pricing` | **Usage pricing.** Versioned `llm.prices/1` books, exact decimal amounts, cache and compute pricing, six separate bases and unknown quantities kept unknown. |
| `spending-limits` | **Durable spending limits.** Opt-in `sqlite`: reservations committed before dispatch, concurrent callers serialized, uncertain charges kept across restart, and compute stop obligations. |

### Serving

| Story | Capability |
| --- | --- |
| `serving-extraction` | **Serving lives in llm-gateway.** The gateway, hosting and provisioning crates and their ESS domains moved to their own repository, llm-gateway, in 0.2.0; llm is the client side. |

### Release

| Story | Capability |
| --- | --- |
| `foundation-qualified` | Pending: **A qualified release.** One exact release tied to its required checks, artifacts and live provider qualification. |
