# Changelog

All notable changes to this component are recorded here. Versions are component-scoped and released
under bare-version tags such as `0.1.0`. The workspace is `publish = false`; releases
are source releases at bare-version tags.

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
