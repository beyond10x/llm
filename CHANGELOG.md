# Changelog

All notable changes to this component are recorded here. Versions are component-scoped and released
under bare-version tags such as `0.1.0`. The workspace is `publish = false`; releases
are source releases at bare-version tags.

## [Unreleased]

## [0.4.0] - 2026-10-08

### Added

- `b10x-llm-models`: `port` builds the `Model` a catalog serving model declares, the binding's
  protocol selecting `ChatClient`, `ResponsesClient` or `MessagesClient` and the account selecting
  the caller's resolver; a credentialed account without one is refused `unauthorized` before any
  I/O. `CatalogModels` builds every serving model's port and implements `llm_routing::Models`.
  `Catalog::bindings` lists the validated bindings. ESS domain `llm.models`, ten new conformance
  cases (702 in all; baseline floor 684).

### Changed

- ESS 0.56.0: the CI `ess` pin and the `ess-conformance` and `ess-primitives` crates.

## [0.3.1] - 2026-10-07

### Fixed

- Responses `server_is_overloaded` errors are temporary `Unavailable` failures eligible for
  caller-owned retries before output, instead of request refusals. Any prior output payload makes
  the failure final, including an opening item without a visible event. Unknown error codes and
  actual refusals remain final; provider prose is never included in diagnostics.

### Changed

- ESS moves to 0.55.0: CI installs the `ess` 0.55.0 release asset after checking it against the
  release's `SHA256SUMS`, and the conformance runner takes `ess-conformance` and `ess-primitives` at
  tag `0.55.0`. The regenerated suite differs only in its provenance (`suite_version`
  `ess-conformance/35`, `scenario_initial_state: empty`); its 691 scenarios, its coverage and the
  schemas are byte-identical.

## [0.3.0] - 2026-10-07

Anthropic subscription access over Messages, and credentials resolved through the secrets library.
No provider route is qualified yet; every guarantee is still held against fixtures.

### Added

- Anthropic subscription access over Messages. `llm-core`'s `AuthKind` gains `SubscriptionOauth`
  (`subscription-oauth`): the caller's own subscription token, resolved through the account's
  `SecretRef` on every turn and never read from a file, presented as a sensitive bearer. A binding
  refuses it on any billing but subscription and any protocol but Messages. For this kind only,
  `llm-messages` sends `anthropic-beta: oauth-2025-04-20` and opens `system` with the subscription
  client preamble (`docs/messages.md`). Parity rows C8, C21, M5, M6 and M44 are covered.
- `llm-routing`: a catalog refuses a secret reference shared between a subscription-oauth account
  and any other account, and a route with fallback that mixes a subscription target with a target
  under other billing, in either order; a subscription token is never presented as a metered
  bearer, and a turn never moves from subscription to metered billing.
- `llm-credentials` feature `secrets`: `SecretsResolver` resolves references by name in one scope
  of a `secrets` v0.5.0 storage (`SecretsResolver::new`) or of its keychain backend
  (`SecretsResolver::keychain`). Every storage error code maps to an existing `SecretError`; the
  version is the backend's version, hashed, or a content hash; a refresh re-reads and is
  `RefreshRejected` while the version is unchanged; a backend without `Read` is
  `UnsupportedPlatform` and is never read. No core crate depends on the library, and the boundary
  tests treat any crate from the secrets source as the library's.
- `llm-credentials`: the example `live_subscription_turn` (features `secrets` and
  `native-keychain`) runs one Messages turn over the operator's own subscription token, read from
  the platform keychain through the secrets library where `secretsctl put` stores it, and prints a
  JSON report: endpoint, auth and billing kind, header names, whether the OAuth beta and the client
  preamble were sent, stop reason, usage, latency or the typed error. `--rotate-check` runs a
  second turn after the token is replaced and reports whether the version changed. The report
  never carries the token. Its logic is tested on a mock store and a loopback fixture; no gate
  runs it live. `docs/live-qualification.md` gives the operator's steps.
- `live_subscription_turn --capture-response <path>` writes the response line, response headers
  and event stream the route sent to a new file, mode 0600 on Unix, and never a request header or
  the token; the report gives the bytes written. It rides on `llm-http`'s new `ResponseTap`
  (`HttpClient::with_response_tap`), which is shown streamed responses only: never the request,
  and never a JSON exchange such as a credential refresh.

### Changed

- `AuthKind` is not `#[non_exhaustive]`, so an exhaustive `match` on it needs a
  `SubscriptionOauth` arm.
- `llm-messages`: a field outside the declared subset is refused with its path, for example
  `Messages field is outside the declared subset: message_start.message.usage.<name>`. The
  name is copied only when it is a bounded name (`?` otherwise); a value never is.
- ESS moves to 0.52.0: CI installs the `ess` 0.52.0 release asset after checking it against the
  release's `SHA256SUMS`, and the conformance runner takes `ess-conformance` and `ess-primitives` at
  tag `0.52.0`. The regenerated suite (665 scenarios) and schemas are byte-identical. The
  conformance suite now runs 691 scenarios (secrets resolver, subscription access and its catalog
  refusals).
- `task rust` also runs clippy on the pinned 1.98.0, the toolchain CI uses.

### Documentation

- The documentation site publishes from this repository at `/llm/` (`pages.yml`,
  `b10x-docs-site.yml`); the generated unified-site bundle, check and redirect workflows and
  `b10x.docs.yaml` are gone.

## [0.2.0] - 2026-10-05

The serving side leaves llm: llm is the client libraries, and serving lives in
[beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway).

### Removed

- The serving crates moved to [beyond10x/llm-gateway](https://github.com/beyond10x/llm-gateway),
  with their tests and history: `b10x-llm-gateway`, `b10x-llm-provision`, `b10x-llm-runpod` and
  `b10x-llm-modal`. The `llm.gateway`, `llm.hosting` and `llm.runpod` ESS domains, their 156
  authored scenarios, `docs/gateway.md`, `docs/hosting.md` and their verification records moved
  with them. llm is the client side; none of its client crates depended on the serving crates, so
  no client API changes. The conformance suite ran 664 scenarios after the move (was 823); its floor is 664.

### Fixed

- `llm-messages`: a `thinking` block whose `content_block_start` carries no `signature` field is
  signed by its `signature_delta`, as Harness accepted; a block never signed and a signature delta
  into any other block are still refused. Found by building Harness on llm 0.1.7; one conformance
  scenario added (665).

### Documentation

- `docs/harness-parity.md`: rows R45 (a forced tool choice answered in prose stays refused,
  deliberately) and M47; row C1 is not needed (credentials come from the secrets library, no token
  files); the subscription rows name the decision to build Anthropic subscription access.

## [0.1.7] - 2026-10-05

Harness parity: `docs/harness-parity.md` maps every public item and test-pinned behaviour of
Harness's model crates to llm (146 rows: covered, partial, gap or deliberately different, each
cited).

### Added

- `llm-credentials`: feature `environment` (`EnvironmentResolver`, caller-named variables only)
  and feature `json-pointer` (`JsonPointerResolver`, RFC 6901 over another resolver). Refusals
  carry the secret reference, never its path, variable or value; `SecretError::Malformed` for a
  document that is not JSON or a target that is not a string.
- `llm-credentials` feature `codex-renewal`: `CodexAuthFile::renew` and `RenewingCodexAuthFile`
  renew a Codex login inside a 15-minute margin through one non-retried JSON POST
  (`HttpClient::post_json`, bounded by `MAX_EXCHANGE_BYTES`), write the token fields back in place
  through a same-directory temporary file with the original mode, refuse a file that changed during
  renewal, a symlink or a hard-linked file, never re-send a refused or uncertain grant for an
  unchanged file, and zeroize the request and the answer.
- `llm-routing`: same-target retry before any visible output (`RetryPolicy`, default 4 attempts,
  1/2/4/8 s back-off, server delay capped at 30 s), then fallback; a `turn-retried` warning before
  each wait; the wait races cancellation; the caller's limit runs before every attempt.
  `llm-core` `Error::retriable` and `may_retry`; `llm-http` marks 408, 429, 5xx, no response, a
  body failing mid-stream and end of stream inside an event as retriable.
- `llm-http`: `CONNECT_TIMEOUT` (15 s) and `HttpClient::with_connect_timeout`.
- `llm-responses`: `encode_request` (the exact request bytes the client sends), opt-in
  conversation identity (`Conversation`, `ResponsesClient::with_conversation`: `prompt_cache_key`,
  `session-id`, `x-client-request-id`) and `request_headers`.
- `llm-messages`: prompt-cache breakpoints on `system` and on the conversation tail; an unknown
  stream event, delta or content block is kept as an opaque item with a warning.
- New crate `b10x-llm-tool-call`: `call_tool` (one forced tool, returns its JSON input) and the
  Codex Responses preset.
- New crate `b10x-llm-blocking`: a blocking adapter over any `Model` for a synchronous loop.

### Changed

- `ResponsesClient` hands each event to the caller as it arrives; text the caller was shown
  through deltas stays in the turn when the terminal output omits it; `keepalive` is not an answer.
- A failure after any Responses output was decoded is final (never replayed).
- `Limits::default().idle` is 180 s (was 60 s).
- `prepare_auth` strips exactly one trailing line terminator from a token, and its refusal names
  the secret reference.
- `FallbackPolicy::disabled()` means one attempt in total.
- A misconfigured Codex login file is refused, not fallen back from: a login read whole that is not
  a Codex login document, or whose access token has no readable integer `exp`, is
  `SecretError::Malformed` (presented as `Unauthorized`, never falls back); a login that ends
  before its document does stays `Unavailable`.
- Breaking: `RenewalRefusal` has a new variant `Malformed` (code `malformed`).
- The workspace lints clean on rustc 1.99 as well as the pinned 1.98.0.

## [0.1.6] - 2026-10-04

### Fixed

- `ResponsesClient` completes a turn against the Codex backend. `llm-http` reads a 2xx with no
  `content-type` as an event stream when the request asked for `text/event-stream` (a 2xx naming
  another type is still refused), and the Responses decoder falls back to the streamed items when
  `response.completed` carries an empty `output` after items were streamed.

## [0.1.5] - 2026-10-04

### Added

- `llm-responses` has a client: `ResponsesClient` implements `Model` over the OpenAI Responses
  wire. It validates the request against its binding, bounds the projected body before the
  credential is resolved, stops at the first terminal event, validates the outcome, and attaches
  the best evidence decoded so far to every refusal after dispatch. All three protocol crates now
  have clients.
- `llm-credentials` feature `codex-auth-file`: `CodexAuthFile` resolves the access token of a Codex
  login's `auth.json` at an explicit absolute path, read-only and on every request. The JWT `exp`
  claim is judged against the caller's clock; renewal is running `codex`. Specified as
  `llm.catalog.CodexAuthFile`.

### Changed

- The AEP store is `aep.project/5`; CI validates it with aep 0.68.0.

## [0.1.4] - 2026-09-27

### Changed

- The specification has no open `UNMAPPED` markers. The gateway's route-count refusal and the
  transport's disabled ambient proxy are declared and covered by scenarios (700, up from 697);
  unbuilt work names its owning story, the gateway's single-owner scope is a recorded decision,
  and the constant-time owner comparison is noted as a property ESS 0.36 cannot express.

## [0.1.3] - 2026-09-27

### Added

- ESS domains `llm.runpod`, `llm.gateway`, `llm.providers` and `llm.transport`, each run by the
  conformance runner against the real crate: 697 scenarios, up from 523.

### Fixed

- The gateway refuses a header name followed by whitespace before its colon with `400
  request-malformed`, as RFC 9112 §5.1 requires; it was trimmed and authenticated.
- The gateway compares SHA-256 digests of the presented and expected owner secrets in constant
  time, so the expected secret's length is not observable through timing. The verifier keeps only
  the digest.

## [0.1.2] - 2026-09-27

### Changed

- The workspace is licensed under Apache-2.0, like the other beyond10x repositories. Releases up to
  0.1.1 remain under `LicenseRef-B10x-Proprietary`.

## [0.1.1] - 2026-09-27

### Changed

- The specification is at ESS source format `ess/14`; the conformance runner uses ESS 0.36.0 and
  CI installs ESS 0.36.0 and AEP 0.61.1.
- The conformance runner parses its command line with clap: `check`, or
  `run SUITE BASELINE OUTPUT_DIR SOURCE_IDENTITY` for one suite.
- `docs/design.md` and `docs/implementation-status.md` state the resolved hosting lifecycle and
  the stories shipped in 0.1.0.

### Added

- 61 authored conformance scenarios from a mutation audit and a refusal-reachability pass (the
  suite runs 523). `llm.secrets.ProbeFile` takes an optional `mode` fixture so each file and
  directory permission rule is reached on its own.

## [0.1.0] - 2026-09-27

### Added

- A mutation audit over the specification and implementation added 38 authored conformance
  scenarios (the suite runs 462); every killable mutant is killed.
- Ordered fallback in `b10x-llm-routing`: `run_turn` tries a route's compatible targets in declared
  order, bounded by attempts and deadline, and halts on visible output, ambiguous dispatch,
  incompatible opaque state, an ineligible failure, cancellation or a caller's limit refusal.
- The Runpod adapter in `b10x-llm-runpod`: single-flight startup, ordered GPU fallback, lost-create
  adoption by a per-controller request id, readiness and crash-window recovery, ownership-safe
  cleanup, proven against an in-process emulator.
- Streamed tool calls carry their announced name (`StreamEvent::ToolCallStarted`) in the Chat,
  Responses and Messages projections.
- Opaque continuation state ingress cannot attribute is carried as `Item::UnattributedOpaque` and
  is never sent until a caller binds it; the envelopes move to `llm.turn/3` and `llm.outcome/4`
  and older versions are refused by name.

- Neutral inference boundary in `b10x-llm-core`: an object-safe asynchronous `Model::turn` port
  taking one request, a caller-owned sink and a cancellation token; bounded text and tool items;
  typed failures with independent dispatch evidence; and opaque continuation state bound to its
  exact protocol, provider, account, endpoint, model and binding revision.
- Injected credential resolution in `b10x-llm-credentials`: an opaque `SecretRef`, request-time
  resolution, zeroized and redacted material with no serialization, and a coordinated resolver that
  refreshes only the credential generation actually rejected.
- Bounded single-attempt transport in `b10x-llm-http`: HTTP and SSE with explicit deadlines, no
  redirects, no automatic retries, and partial output preserved across failure.
- Provider bindings in `b10x-llm-providers`: independent protocol, provider, authentication and
  billing choices, including anonymous arbitrary endpoints.
- TOML catalog routing in `b10x-llm-routing`: strict `llm.catalog/1` parsing, deterministic
  configuration identity, ordered opt-in selection, capability and input-token admission, and a
  safe explanation that resolves no secret and performs no request.
- Attributed usage pricing in `b10x-llm-cost`: versioned `llm.prices/1` books, exact decimal
  arithmetic over `llm.usage/2` observations, `llm.cost/2` reports with six separate accounting
  bases, and unknown quantities preserved as unknown rather than zero.
- Optional local secret adapters behind the `file`, `keychain` and `native-keychain` features:
  explicitly mapped protected files on Linux and an explicitly injected or native credential store.
- Optional durable spending admission behind `b10x-llm-cost`'s `sqlite` feature: an immutable
  single-owner `BudgetPolicy`, reservations committed before dispatch, serialized concurrent
  callers, cross-process owner exclusion, uncertain charges retained across restart, and compute
  stop obligations.
- The Responses, Messages and Chat Completions protocol projections in `b10x-llm-responses`,
  `b10x-llm-messages` and `b10x-llm-chat`, each for its declared supported subset: independent
  authentication, billing and protocol choices survive translation; unsupported fields and
  provider-specific opaque continuation state are preserved or refused rather than dropped; and
  each adapter normalizes its wire's usage counts before producing neutral values, reporting a
  partial quantity only where it is a valid lower bound.
- An authenticated single-owner gateway in `b10x-llm-gateway`: nothing is decoded past the HTTP
  head before an owner verifier accepts, apart from a closed liveness and readiness surface; the
  route inventory is an immutable snapshot with no field for an endpoint URL, secret reference or
  credential material; and start, drain and stop are deliberate. It performs no protocol
  translation, proxies no model call, resolves no secret and reaches no network.
- The hosting lifecycle contract in `b10x-llm-provision`, with an in-process `FakeProvider` that
  demonstrates it: resource identity qualified by the provider's `incarnation` so a recycled name
  cannot be adopted as the same resource, requested state kept strictly separate from observed
  state, and a stop obligation that nothing but evidence discharges. It opens no socket, reads no
  credential and allocates no cloud resource.
- Executable ESS verification for the `accounting`, `budget`, `chat`, `hosting`, `inference`,
  `messages`, `responses`, `routing` and `secrets` domains — every domain except `catalog`, which
  declares rather than observes — with recorded falsification evidence under `docs/verification/`.
  Two naming notes: the suite directory `contracts/pricing/` exercises the `accounting` domain and
  there is no `pricing` domain; and `gateway-auth` has a falsification record but no ESS suite.
- Public documentation surface: a `b10x-docs/v4` manifest, a Docusaurus documentation tree under
  `website/`, this changelog, a proprietary licence notice, the shared Gates caller workflow and a
  `b10x-change/v1` feed entry.

### Changed

- The specification is at ESS source format `ess/13` and the conformance runner uses ESS 0.35.0;
  emitted payload fields the adapters observe are declared `generated`.
- The planning store is `aep.project/3` on a tree Git merges, planned with AEP 0.60.0; CI installs
  AEP 0.60.0.
- The workspace version is 0.1.0.
- Pin every action revision in `.github/workflows/gate.yml` to an exact commit and name the
  version beside it. Common Gates refuses a workflow whose action revisions are not exact commits
  or Docker digests, so the repository's own gate had to be pinned before the shared caller could
  be enabled. No step, trigger, permission or toolchain version changed: `1.98.0` moved from the
  `dtolnay/rust-toolchain` ref to its `toolchain:` input, which is how the action takes it once
  the ref is a commit.

### Fixed

- Keep transport deadline and dispatch evidence deterministic.
- Release the spending ledger's owner lock after connection close.
- Compile the spending ledger's directory synchronization only on Unix.
- Preserve bound inference observations and partial usage costs.

### Not yet implemented

The operator command line (`llm-cli`), the Modal hosting adapter (`llm-modal`) and protocol
translation in the gateway are not written yet. OpenAI and Anthropic access is unqualified: no live provider credential has been used
anywhere in this repository, so every guarantee above is held against fixtures, local sockets and
in-process fakes. No crate is published to a registry.
