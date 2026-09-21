# Changelog

All notable changes to this component are recorded here. Versions are component-scoped and released
under bare-version tags such as `0.1.0`. Nothing has been released yet: the workspace is
`publish = false` at version `0.0.0`, and every entry below is unreleased.

## [Unreleased]

### Added

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

The operator command line (`llm-cli`), both cloud hosting adapters (`llm-runpod`, `llm-modal`),
protocol translation in the gateway, and ordered runtime fallback — all still five-line stubs or
unwritten. OpenAI and Anthropic access is unqualified: no live provider credential has been used
anywhere in this repository, so every guarantee above is held against fixtures, local sockets and
in-process fakes. There is no release and no published artifact.
