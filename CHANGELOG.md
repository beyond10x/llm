# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions are workspace-wide source releases at bare tags such as `0.8.3`; the workspace is `publish = false`.

## [Unreleased]

### Added

- `llm-cli`: the `llm` command, `llm model list` (text or JSON) and `llm infer` over one `llm.toml` routes file
- `llm-cli`: `llm infer` streams the text to stdout and ends with usage and cost on stderr; unknown counters print `unknown`
- `llm-cli`: the file is `--config`, `LLM_CONFIG`, `~/.b10x/llm.toml` or `$XDG_CONFIG_HOME/b10x/llm.toml`, in that order
- `llm-models`: `Routes::parse_toml` reads `llm.routes/1` written in TOML, with the same keys, refusals and digest
- `llm-models`: route `aliases` and a per-million-token `price`; `Routes::cost` prices a turn, unknown counts stay unknown
- `llm-models`: Bedrock endpoints (`bedrock.region`, `aws-keys` or `aws-container`); `Routes::port` builds their client
- `llm-models`: `SecretSources::with_container_credentials` supplies the container endpoint and its token file
- `llm-cost`: `PriceDocument::new` builds a price book in code
- `llm-cli`: `keychain:` references read the platform keychain, opened only when the asked route needs it
- `llm-credentials`: `FileResolver::mounted` reads a platform-mounted file such as a Kubernetes projected volume (unverified)
- `llm-models`: `Resolvers::unavailable` lets a refused port say why its account has no resolver
- ESS domain `llm.routes`: 5 authored scenarios for aliases and Bedrock endpoints; the conformance floor rises from 937 to 942

### Changed

- `llm-models`: an `llm.routes/1` document may name `bedrock-converse` on a Bedrock endpoint; a binding and a catalog still refuse it
- **Breaking:** `llm-models`: `Routes::resolvers` refuses an unbuildable source per endpoint, as `unauthorized` at `port`
- `llm-models`: an `aws-container` token file is read through `FileResolver::mounted`, so a projected token is accepted

### Fixed

- `llm-cli`: a reader that closes stdout stops the turn with exit 0 instead of a panic (101) or a false Ctrl-C (130)
- `llm-cli`: a second Ctrl-C drops a turn the first did not end, with exit 130

## [0.11.0] - 2026-10-22

### Added

- `llm-bedrock`: Amazon Bedrock Runtime behind `Model::turn`, over Converse and Anthropic `InvokeModel`; fixture-checked only
- `llm-bedrock`: Signature Version 4 signing on `ring` and AWS event-stream decoding, with no AWS SDK crate and no `aws-lc`
- `llm-bedrock`: `StaticCredentials` through an injected resolver and `ContainerCredentials` (EKS Pod Identity, unverified)
- `llm-core`: `Protocol::BedrockConverse`; a binding, catalog or routes document refuses it by name
- `llm-core`: `BEDROCK_CONVERSE_REFUSAL`, the refusal a binding and a base URL both give that protocol
- `llm-http`: `ByteStream`, a bounded raw byte stream for wires that frame their own messages
- `llm-http`: `HttpClient::get_json`, a bounded GET that decodes the answer into the caller's type
- ESS domain `llm.bedrock` with 43 authored scenarios; the conformance floor rises from 890 to 937

## [0.10.0] - 2026-10-22

### Changed

- **Breaking:** every package drops the `b10x-` prefix: `b10x-llm-core` is `llm-core`, and so on for all 14 packages
- **Breaking:** `llm-tool-call`'s library is `llm_tool_call` (was `b10x_llm_tool_call`); other library names are unchanged
- A consumer writes `llm-core = { git = "…", tag = "…" }` without `package = "b10x-llm-core"`

## [0.9.0] - 2026-10-21

### Added

- `b10x-llm-responses`: a live Codex text turn (`gpt-6-astra`, 2026-10-21), scrubbed, as a replayed fixture
- `b10x-llm-messages`: two live Messages turns (`claude-haiku-4-5`, 2026-10-21), a cache write and a cache read of 13027 tokens, scrubbed, as replayed fixtures

### Changed

- **Breaking:** `b10x-llm-responses`: `ingest_request` refuses a `function` tool without a `parameters` key (`InvalidRequest`), as the chat ingress does; a present `null` is still refused by the schema check
- **Breaking:** `b10x-llm-chat`: `decode_ingress_request` refuses a `text` content part whose `text` is absent or `null` (`InvalidRequest`); an empty string is still text
- The conformance check and CI run ess 0.67.0 (was 0.66.0)

## [0.8.4] - 2026-10-21

### Fixed

- `b10x-llm-chat`: llama-server `timings` counts the turn only on the report ending the stream; earlier ones are dropped

## [0.8.3] - 2026-10-21

### Added

- `b10x-llm-credentials`: `AnonymousResolver` for anonymous bindings, refusing every resolve with `SecretError::Missing`
- `b10x-llm-http`: `require_tokio_runtime` and `within_tokio_runtime`, which refuse a wait outside a Tokio runtime
- `b10x-llm-chat`: three llama-server stream fixtures (reasoning text, tool call, no usage chunk) and five ESS scenarios
- `docs/design.md` "Input into a running turn" and the offline guide "Interrupt a turn with new input"

### Changed

- `b10x-llm-models`: an anonymous account's client gets `AnonymousResolver` in place of a private copy
- `b10x-llm-messages`: a missing `cache_creation_input_tokens` is the sum of both `cache_creation` TTL counts
- `b10x-llm-chat`: llama-server `timings` fills the input, cached input and output counters `usage` does not name
- `b10x-llm-chat`: `usage` and `timings` that disagree fail as `chat completion timings contradict its usage report`
- CI and the conformance runner use ess 0.66.0 (was 0.56.0); `contracts/suite.json` is regenerated (888 scenarios)

### Fixed

- Protocol clients and `b10x-llm-http` exchanges polled outside a Tokio runtime fail with `unsupported`, not a panic

## [0.8.2] - 2026-10-20

### Added

- `b10x-llm-http`: `QuotaRule`, `HttpClient::with_quota_rule`, `MAX_REFUSAL_BYTES` (16 KiB) and `REFUSAL_WAIT` (1 s)

### Changed

- An HTTP 429 that reports an exhausted quota, spend limit or usage limit is final: `refused`, dispatch `rejected`
- `b10x-llm-responses`: a 429 with an OpenAI quota or spend `error.code`, or a Codex usage-limit `error.type`, is final
- `b10x-llm-messages`: a 429 `rate_limit_error` with `error.details.error_code` `enforced_spend_limit_reached` is final
- `b10x-llm-http`: an error answer carrying `x-should-retry: false` is not retriable, whatever its status

### Fixed

- `b10x-llm-messages`, `b10x-llm-chat`: a stream cut after output the caller was not shown is final, as on Responses

## [0.8.1] - 2026-10-20

### Fixed

- `b10x-llm-messages`: a response carrying `container` is accepted; 0.8.0 refused it as `Unsupported`
- `b10x-llm-messages`: documented default `diagnostics`, `stop_details`, `speed`, `toolset_name`, `caller` are accepted

## [0.8.0] - 2026-10-20

### Added

- `b10x-llm-core`: `Capabilities` fields `output_streaming` and `input_during_output`, with `with_*` setters
- `llm.binding/1`, `llm.catalog/1` and `llm.routes/1` take both fields, optional and not written at their defaults
- `b10x-llm-core`: `Capabilities::validate_protocol`, called by `BindingDocument::bind` and `Routes::parse`
- `b10x-llm-core`: `SHOWN_TEXT_KEPT` and `SHOWN_TEXT_UNPAIRED`; streamed events are documented as final
- A call the caller rejected before running it is sent as `failed: true` with the reason in `output`
- 15 conformance scenarios for streaming declarations, shown text and the Chat failure object (883 in all)

### Changed

- **Breaking:** `b10x-llm-core`: `Capabilities` has two more public fields, so a full struct literal no longer compiles
- `b10x-llm-models`: a string route's default capabilities are stated per field, not as "every boolean true"
- `b10x-llm-responses`: a streamed turn's assistant text is the text deltas the caller was shown
- `b10x-llm-responses`: `ResponsesClient` warns `shown-text-kept` or `shown-text-unpaired` about shown text
- `b10x-llm-responses`: `server_error` and `rate_limit_exceeded` are retriable before any output, final after it
- `b10x-llm-responses`: `response.queued` is in `ACCEPTED_STREAM_EVENTS` and ignored like `response.in_progress`
- `b10x-llm-messages`: text a `text` block opens with in `content_block_start` is shown as a `StreamEvent::TextDelta`
- `b10x-llm-messages`: stream errors `overloaded_error` and `api_error` are retriable before any output, final after it

### Fixed

- `b10x-llm-chat`: gateway ingress reads a tool message `{"error": <output>, "ok": false}` as a failed result

## [0.7.0] - 2026-10-17

### Added

- `b10x-llm-core`: `TurnRequest.controls` (`Controls`: `thinking`, `parallel_tool_calls`, `hints`) and `wire_effort`
- `b10x-llm-core`: `Advisory`, `Hints::omitted_by`, `advisory_warning` and `field_path`
- `b10x-llm-core`: a turn with controls is the envelope `llm.turn/4`; one without stays `llm.turn/3`
- Each protocol crate: `omitted_advisories(&TurnRequest)`; its client warns `advisory-omitted` after a successful turn
- `b10x-llm-messages`: ingress carries `thinking`, a text-block `system`, `cache_control` and effort `max`
- `b10x-llm-messages`: egress re-emits thinking and the parallel choice, and can give breakpoints a `1h` lifetime
- `b10x-llm-responses`: `instructions`, `parallel_tool_calls`, `prompt_cache_key`, `text.verbosity`, `reasoning.summary`
- `b10x-llm-responses`: Codex 0.160.0 first- and second-turn fixtures, captured against a loopback recorder
- `b10x-llm-chat`: `parallel_tool_calls`, `prompt_cache_key` and `verbosity` are carried both ways
- 91 conformance scenarios (868 in all) and a cross-wire test projecting each client's first turn onto the other wires

### Changed

- `b10x-llm-core`: `TurnRequest::validate_for` checks `reasoning_efforts` against the effort the wire receives
- `b10x-llm-chat`, `b10x-llm-responses`: an unknown request key is refused naming its path at every level
- `b10x-llm-chat`: the top-level refusal reads `chat request field is outside the published subset: <path>`
- `b10x-llm-responses`: the top-level refusal reads `request body carries a field outside the pinned subset: <path>`
- `b10x-llm-chat`: a `strict` function schema is refused instead of dropped
- `b10x-llm-responses`: a hosted or custom tool is refused naming its type
- The shared source gates workflow runs Gates 0.1.21 (was 0.1.3)

## [0.6.0] - 2026-10-16

### Added

- `b10x-llm-models`: model routes, format `llm.routes/1` (YAML, `llm_models::routes`), read by `Routes::parse`
- `b10x-llm-models`: `Routes::port` and `Routes::resolvers(&SecretSources)`, building ports and `RouteResolvers`
- `b10x-llm-models`: default-off features `environment`, `file` and `keychain` for route secret references
- ESS domain `llm.routes` and 24 scenarios
- `b10x-llm-routing`: `CatalogDocument::new`, so a catalog can be built from declarations
- `b10x-llm-responses`: `encode_ingress_response` and `IngressStream`, the answer half of gateway ingress
- `b10x-llm-messages`: `encode_ingress_message` and `IngressStream`, the answer half of gateway ingress
- ESS commands `llm.responses.EncodeIngress` (14 scenarios) and `llm.messages.EncodeIngress` (11 scenarios)

### Changed

- The conformance suite answers 777 scenarios and skips none (baseline floor 705 to 777)

## [0.5.1] - 2026-10-08

### Changed

- The status page lists the Secrets resolver, catalog model port, provider description and written contract as shipped
- Pages, README.md and AGENTS.md link llm-gateway's repository instead of describing serving

## [0.5.0] - 2026-10-08

### Added

- `b10x-llm-providers`: provider descriptions, format `llm.provider-description/1` (TOML, at most 64 KiB)
- `b10x-llm-providers`: `ProviderDescription::parse` and `inference_base_url`
- `b10x-llm-providers`: Runpod's description as `descriptions::runpod()`
- ESS command `llm.providers.Describe` and 21 conformance scenarios (baseline floor 704)

## [0.4.0] - 2026-10-08

### Added

- `b10x-llm-models`: `port` builds a catalog serving model's `Model` from its binding's protocol and account
- `b10x-llm-models`: `CatalogModels` builds every serving model's port and implements `llm_routing::Models`
- `b10x-llm-routing`: `Catalog::bindings` lists the validated bindings
- ESS domain `llm.models` and ten conformance cases (702 in all; baseline floor 684)

### Changed

- ESS 0.56.0: the CI `ess` pin and the `ess-conformance` and `ess-primitives` crates

## [0.3.1] - 2026-10-07

### Changed

- ESS 0.55.0 in CI and the conformance runner; the regenerated suite differs only in its provenance

### Fixed

- `b10x-llm-responses`: `server_is_overloaded` is a temporary `Unavailable` failure, retriable before any output

## [0.3.0] - 2026-10-07

### Added

- `b10x-llm-core`: `AuthKind::SubscriptionOauth` (`subscription-oauth`) for Anthropic subscription access over Messages
- `b10x-llm-messages`: a `subscription-oauth` turn sends `anthropic-beta: oauth-2025-04-20` and the client preamble
- `b10x-llm-routing`: a catalog refuses a secret reference shared by a subscription-oauth account and another account
- `b10x-llm-routing`: a catalog refuses a fallback route that mixes subscription and other billing
- `b10x-llm-credentials`: feature `secrets` with `SecretsResolver` over a `secrets` v0.5.0 storage or its keychain
- `b10x-llm-credentials`: example `live_subscription_turn`, one Messages turn over the operator's subscription token
- `b10x-llm-credentials`: `live_subscription_turn --capture-response <path>` writes the route's response to a new file
- `b10x-llm-http`: `ResponseTap` and `HttpClient::with_response_tap`, shown streamed responses only

### Changed

- **Breaking:** `b10x-llm-core`: an exhaustive `match` on `AuthKind` needs a `SubscriptionOauth` arm
- `b10x-llm-messages`: a field outside the declared subset is refused with its path
- ESS 0.52.0 in CI and the conformance runner; the suite runs 691 scenarios
- `task rust` also runs clippy on the pinned 1.98.0
- The documentation site publishes from this repository at `/llm/` (`pages.yml`, `b10x-docs-site.yml`)

### Removed

- The unified-site bundle, check and redirect workflows and `b10x.docs.yaml`

## [0.2.0] - 2026-10-05

### Changed

- `docs/harness-parity.md`: rows R45 and M47; row C1 is not needed

### Removed

- **Breaking:** `b10x-llm-gateway`, `b10x-llm-provision`, `b10x-llm-runpod`, `b10x-llm-modal` moved to llm-gateway
- ESS domains `llm.gateway`, `llm.hosting` and `llm.runpod` (156 scenarios) moved to llm-gateway; the suite floor is 664

### Fixed

- `b10x-llm-messages`: a `thinking` block opened without a `signature` is signed by its `signature_delta`

## [0.1.7] - 2026-10-05

### Added

- `docs/harness-parity.md` maps every public item and test-pinned behaviour of Harness's model crates to llm (146 rows)
- `b10x-llm-credentials`: features `environment` (`EnvironmentResolver`) and `json-pointer` (`JsonPointerResolver`)
- `b10x-llm-credentials`: feature `codex-renewal` with `CodexAuthFile::renew` and `RenewingCodexAuthFile`
- `b10x-llm-routing`: same-target retry before visible output (`RetryPolicy`), then fallback; `turn-retried` warnings
- `b10x-llm-core`: `Error::retriable` and `may_retry`
- `b10x-llm-http`: 408, 429, 5xx, no response and a body failing mid-stream are marked retriable
- `b10x-llm-http`: `CONNECT_TIMEOUT` (15 s) and `HttpClient::with_connect_timeout`
- `b10x-llm-responses`: `encode_request`, `request_headers` and opt-in conversation identity (`Conversation`)
- `b10x-llm-messages`: prompt-cache breakpoints on `system` and on the conversation tail
- `b10x-llm-messages`: an unknown stream event, delta or content block is kept as an opaque item with a warning
- New crate `b10x-llm-tool-call`: `call_tool` and the Codex Responses preset
- New crate `b10x-llm-blocking`: a blocking adapter over any `Model` for a synchronous loop

### Changed

- **Breaking:** `b10x-llm-credentials`: `RenewalRefusal` has a new variant `Malformed` (code `malformed`)
- `b10x-llm-responses`: `ResponsesClient` hands each event to the caller as it arrives; `keepalive` is not an answer
- `b10x-llm-responses`: text shown through deltas stays in the turn when the terminal output omits it
- `b10x-llm-responses`: a failure after any decoded output is final, never replayed
- `b10x-llm-http`: `Limits::default().idle` is 180 s (was 60 s)
- `b10x-llm-providers`: `prepare_auth` strips one trailing line terminator; its refusal names the secret reference
- `b10x-llm-routing`: `FallbackPolicy::disabled()` means one attempt in total
- `b10x-llm-credentials`: a misconfigured Codex login file is `SecretError::Malformed` and never falls back
- The workspace lints clean on rustc 1.99 as well as the pinned 1.98.0

## [0.1.6] - 2026-10-04

### Fixed

- `b10x-llm-http`: a 2xx with no `content-type` reads as an event stream when the request asked for `text/event-stream`
- `b10x-llm-responses`: an empty `output` in `response.completed` after streamed items falls back to those items

## [0.1.5] - 2026-10-04

### Added

- `b10x-llm-responses`: `ResponsesClient` implements `Model` over the OpenAI Responses wire
- `b10x-llm-credentials`: feature `codex-auth-file`: `CodexAuthFile` reads a Codex login's `auth.json` access token

### Changed

- The AEP store is `aep.project/5`; CI validates it with aep 0.68.0

## [0.1.4] - 2026-09-27

### Changed

- The specification has no open `UNMAPPED` markers; 700 scenarios, up from 697

## [0.1.3] - 2026-09-27

### Added

- ESS domains `llm.runpod`, `llm.gateway`, `llm.providers` and `llm.transport`: 697 scenarios, up from 523

### Fixed

- `b10x-llm-gateway`: a header name followed by whitespace before its colon is refused with `400 request-malformed`
- `b10x-llm-gateway`: owner secrets are compared as SHA-256 digests in constant time

## [0.1.2] - 2026-09-27

### Changed

- The workspace is licensed under Apache-2.0; releases up to 0.1.1 remain under `LicenseRef-B10x-Proprietary`

## [0.1.1] - 2026-09-27

### Added

- 61 authored conformance scenarios from a mutation audit and a refusal-reachability pass (523 in all)
- `llm.secrets.ProbeFile` takes an optional `mode` fixture

### Changed

- The specification is at ESS source format `ess/14`; CI installs ESS 0.36.0 and AEP 0.61.1
- The conformance runner parses its command line with clap: `check`, or `run SUITE BASELINE OUTPUT_DIR SOURCE_IDENTITY`
- `docs/design.md` and `docs/implementation-status.md` state the resolved hosting lifecycle

## [0.1.0] - 2026-09-27

### Added

- `b10x-llm-core`: the neutral inference boundary, an object-safe asynchronous `Model::turn` port
- `b10x-llm-credentials`: injected credential resolution with an opaque `SecretRef` and zeroized, redacted material
- `b10x-llm-credentials`: optional local secret adapters behind the `file`, `keychain` and `native-keychain` features
- `b10x-llm-http`: bounded single-attempt HTTP and SSE transport with explicit deadlines and no automatic retries
- `b10x-llm-providers`: provider bindings with independent protocol, provider, authentication and billing choices
- `b10x-llm-routing`: TOML catalog routing (`llm.catalog/1`) and ordered fallback in `run_turn`
- `b10x-llm-cost`: usage pricing with `llm.prices/1` books, `llm.usage/2` observations and `llm.cost/2` reports
- `b10x-llm-cost`: durable spending admission behind the `sqlite` feature (`BudgetPolicy`)
- `b10x-llm-responses`, `b10x-llm-messages`, `b10x-llm-chat`: the three protocol projections for their declared subsets
- Streamed tool calls carry their announced name (`StreamEvent::ToolCallStarted`) in all three projections
- `Item::UnattributedOpaque` for opaque state ingress cannot attribute; envelopes `llm.turn/3` and `llm.outcome/4`
- `b10x-llm-gateway`: an authenticated single-owner gateway
- `b10x-llm-provision`: the hosting lifecycle contract with an in-process `FakeProvider`
- `b10x-llm-runpod`: the Runpod adapter, proven against an in-process emulator
- ESS verification for nine domains, with falsification evidence under `docs/verification/` (462 scenarios)
- Documentation: a `b10x-docs/v4` manifest, a Docusaurus tree under `website/` and this changelog

### Changed

- The specification is at ESS source format `ess/13` and the conformance runner uses ESS 0.35.0
- The planning store is `aep.project/3`, planned with AEP 0.60.0; CI installs AEP 0.60.0
- Every action revision in `.github/workflows/gate.yml` is pinned to an exact commit

### Fixed

- `b10x-llm-http`: transport deadline and dispatch evidence are deterministic
- `b10x-llm-cost`: the spending ledger releases its owner lock after connection close
- `b10x-llm-cost`: the spending ledger's directory synchronization compiles only on Unix
- Bound inference observations and partial usage costs are preserved

[Unreleased]: https://github.com/beyond10x/llm/compare/0.11.0...HEAD
[0.11.0]: https://github.com/beyond10x/llm/compare/0.10.0...0.11.0
[0.10.0]: https://github.com/beyond10x/llm/compare/0.9.0...0.10.0
[0.9.0]: https://github.com/beyond10x/llm/compare/0.8.4...0.9.0
[0.8.4]: https://github.com/beyond10x/llm/compare/0.8.3...0.8.4
[0.8.3]: https://github.com/beyond10x/llm/compare/0.8.2...0.8.3
[0.8.2]: https://github.com/beyond10x/llm/compare/0.8.1...0.8.2
[0.8.1]: https://github.com/beyond10x/llm/compare/0.8.0...0.8.1
[0.8.0]: https://github.com/beyond10x/llm/compare/0.7.0...0.8.0
[0.7.0]: https://github.com/beyond10x/llm/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/beyond10x/llm/compare/0.5.1...0.6.0
[0.5.1]: https://github.com/beyond10x/llm/compare/0.5.0...0.5.1
[0.5.0]: https://github.com/beyond10x/llm/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/beyond10x/llm/compare/0.3.1...0.4.0
[0.3.1]: https://github.com/beyond10x/llm/compare/0.3.0...0.3.1
[0.3.0]: https://github.com/beyond10x/llm/compare/0.2.0...0.3.0
[0.2.0]: https://github.com/beyond10x/llm/compare/0.1.7...0.2.0
[0.1.7]: https://github.com/beyond10x/llm/compare/0.1.6...0.1.7
[0.1.6]: https://github.com/beyond10x/llm/compare/0.1.5...0.1.6
[0.1.5]: https://github.com/beyond10x/llm/compare/0.1.4...0.1.5
[0.1.4]: https://github.com/beyond10x/llm/compare/0.1.3...0.1.4
[0.1.3]: https://github.com/beyond10x/llm/compare/0.1.2...0.1.3
[0.1.2]: https://github.com/beyond10x/llm/compare/0.1.1...0.1.2
[0.1.1]: https://github.com/beyond10x/llm/compare/0.1.0...0.1.1
[0.1.0]: https://github.com/beyond10x/llm/releases/tag/0.1.0
