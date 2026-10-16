---
format: aep.planning-md/3
id: story:routes-document
kind: story
status: active
title: One llm.routes/1 document yields the catalog and the resolvers every consumer builds from
summary: YAML routes expand into llm.catalog/1 and llm_models::Resolvers from env/file/keychain references, strictly
relations:
- decomposes: epic:routes-and-ingress
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: checks/conformance/Cargo.toml
- confidence: cited
  path: checks/conformance/src/main.rs
- confidence: cited
  path: checks/conformance/src/routes.rs
- confidence: cited
  path: checks/conformance/src/target.rs
- confidence: cited
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/routes/scenarios
- confidence: cited
  path: crates/llm-models/Cargo.toml
- confidence: cited
  path: crates/llm-models/src
- confidence: cited
  path: crates/llm-models/tests
- confidence: cited
  path: crates/llm-routing/src/catalog.rs
- confidence: cited
  path: spec/domains/routes.yaml
revision: 18
transitions:
- {from: "draft", to: "proposed", at: "2026-10-16T02:36:55Z", actor: "human:timo", revision: 14, decided_on: {"recorded":{"review_outcome":5}}}
- {from: "proposed", to: "active", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 15, decided_on: {"recorded":{"review_outcome":5}}}
---
## Context

Model reach is configured today with an `llm.catalog/1` TOML catalog (`crates/llm-routing/src/catalog.rs`):
providers, accounts, endpoints, models, serving models, routes and targets, each declared by id. That
is the right internal model and too long for an operator writing which model a CLI should use. The
gateway, Loom CLI and Loom SDK need one short document they all read the same way. The entity is
modelled in `spec/domains/routes.yaml` (domain `llm.routes`).

## Design

The document (YAML, at most `MAX_CONFIG_BYTES`):

```yaml
format: llm.routes/1
providers:
  anthropic:  {base_url: https://api.anthropic.com/v1, protocol: messages,
               auth: {kind: api-key, header: x-api-key, secret: env:ANTHROPIC_API_KEY}}
  openrouter: {base_url: https://openrouter.ai/api/v1, protocol: chat-completions,
               auth: {kind: bearer, secret: env:OPENROUTER_API_KEY}}
  gateway:    {base_url: https://gateway.internal/v1, protocol: responses,
               auth: {kind: bearer, secret: file:~/.config/b10x/gateway-token}}
routes:
  sonnet:    anthropic/claude-sonnet-4-5
  or-sonnet: openrouter/anthropic/claude-sonnet-4.5
  qwen:      gateway/qwen
```

- Home: module `routes` of `b10x-llm-models` (`llm_models::routes`), because it expands into the
  catalog `llm_models::port` and `CatalogModels::build` already consume and builds the
  `llm_models::Resolvers` that crate already defines. `b10x-llm-routing` gains only a public
  constructor for `CatalogDocument` (its `format` field is private); routing still depends on no
  protocol crate (`crates/llm-core/tests/dependency_boundary.rs`).
- YAML parser: `serde_yaml` 0.9, already in `Cargo.lock` through ESS, so no new package enters the
  lockfile.
- `protocol`: `messages`, `responses`, `chat-completions`. `auth.kind`: `anonymous`, `bearer`,
  `api-key` (`header` required exactly for `api-key`). `subscription-oauth` is refused by name.
  Billing is `metered` for a credentialed provider and `self-hosted` for an anonymous one.
- `auth.secret` is a reference only: `env:<VARIABLE>`, `file:<absolute path or ~/path>`,
  `keychain:<service>/<entry>`. Any other prefix, or a bare value, is refused; required for
  `bearer`/`api-key`, refused for `anonymous`.
- A route value splits on its first `/` only: `openrouter/anthropic/claude-sonnet-4.5` is provider
  `openrouter`, model `anthropic/claude-sonnet-4.5`. Route and provider names hold no `/`.
- A name that is not a route but reads `<declared provider>/<model-id>` resolves directly.
- Expansion: per provider one `Provider`, `Account` and `Endpoint`, all with the provider's name as
  id; the account's secret reference id is the provider's name. Per route one `ServedModel`, one
  `ServingModel` and one single-target catalog route, ids from the route name. Capabilities: a route
  may be written as a mapping `{model: <provider>/<model-id>, capabilities: {...}}`; otherwise a
  fixed default (every boolean true, reasoning efforts `low`, `medium`, `high`, context window
  200000, maximum output 16384), documented on the routes page. The mapping form goes
  beyond the approved example because capabilities gate every request (`crates/llm-core/src/turn.rs:233-249`)
  and Messages sends `max_output_tokens` as its default `max_tokens` (`crates/llm-messages/src/codec.rs:140`),
  so one fixed default cannot fit every model; it is a deviation the release report names.
- Resolvers: built from the references, one resolver per credentialed provider through
  `EnvironmentResolver`, `FileResolver` and `KeychainResolver` (`b10x-llm-credentials` features
  `environment`, `file`, `keychain`, forwarded as same-named, default-off features of
  `b10x-llm-models`). Building reads nothing. A reference whose feature is off, or a `keychain:` one
  without an injected store, is refused by name when the resolvers are built. `~/` expands against a
  home directory the caller passes in; nothing reads `HOME` implicitly.
- No ambient fallback: at turn time an unset variable or a missing file is refused as
  `unauthorized`, naming the provider's reference and never the variable, path or value, before any
  request is sent.
- Strict: unknown fields at every level, an unknown `format`, a route naming an undeclared provider,
  an unknown protocol or auth kind, a duplicate key and an unknown secret source refuse with a named
  `invalid-request` error that echoes no value.

## Shared files

This story edits, besides its own crate, test and scenario files: `spec/domains/routes.yaml` (written
by the coordinator at wave open; refined here only if the adapter needs it), its own lines in
`contracts/ess-inputs.yaml` (the `routes/scenarios/` block), `checks/conformance/src/{main,target}.rs`
to register a `routes` module, `checks/conformance/Cargo.toml` (features) and `Cargo.lock`.
story:responses-ingress-encoder and story:messages-ingress-encoder append to the same
`contracts/ess-inputs.yaml` in their own domains' blocks. `contracts/suite.json`, `contracts/schema/`
and `contracts/baseline.json` are regenerated or raised in the branch only to run the check there;
the coordinator owns them at integration: it resolves `ess-inputs.yaml`, regenerates the
projections and sets the baseline once on the merged wave. The conformance acceptance below is read
on the merged wave.
The CHANGELOG.md (Unreleased), README.md, AGENTS.md, docs/implementation-status.md with `task docs-generate`, the hand-written pages (docs/responses.md, docs/messages.md, the routes page) and the website pages are written by the coordinator once on the integration branch after the three stories merge, not in this story's branch.

## Acceptance

Conformance scenarios in `contracts/routes/scenarios/` over the `llm.routes` domain, all passing in
`task conformance` on the merged wave with no skips and `contracts/baseline.json` raised by their
count:

- the three-provider example above loads; each route answers its provider, protocol and upstream
  model, and the expanded catalog's serving models are the routes;
- `or-sonnet` answers provider `openrouter`, model `anthropic/claude-sonnet-4.5` (first `/` only);
- an undeclared name `anthropic/claude-opus-4-1` answers provider `anthropic` directly; a name
  `nobody/model` naming an undeclared provider, and a bare unknown name, answer `invalid-request`;
- a route written as a mapping with `capabilities` answers with those capabilities in the expanded
  catalog, as the `capabilities` field of `llm.routes.Loading` reports them
  (`qwen=tools:true,tool_choice:true,temperature:true,top_p:true,efforts:,context:32768,output:8192`),
  and a string route reports the default
  (`sonnet=tools:true,tool_choice:true,temperature:true,top_p:true,efforts:low|medium|high,context:200000,output:16384`);
- each of: an unknown top-level field, an unknown provider field, an unknown `auth` field,
  `format: llm.routes/2`, a route to an undeclared provider, `protocol: grpc`, `kind: password`,
  `kind: subscription-oauth`, a secret `sk-literal` with no source prefix, `vault:x`, an `api-key`
  provider without `header`, an `anonymous` provider with a `secret`, and a provider declared twice
  (duplicate YAML key) is refused as `invalid-request` without loading, and the error message
  contains none of the refused value's text;
- authenticating a route whose `env:` variable is unset is refused `unauthorized` naming the
  provider's reference, and the message carries neither the variable name nor a value; a `file:`
  reference to a missing file likewise; a present `file:` secret prepares the `api-key` header the
  provider names, and an anonymous provider prepares nothing and succeeds.

Unit tests in `crates/llm-models/tests/routes.rs`, each a named `#[test]`:

- building resolvers for a `keychain:` reference without an injected store is refused
  `invalid-request` naming the provider;
- with the `file` feature, `file:~/x` expands against the home the caller passes, and without a home
  it is refused; a relative `file:` path is refused;
- each source kind whose feature is compiled out is refused by name when resolvers are built
  (a `#[cfg(not(feature = ...))]` test per feature);
- `CatalogModels::build` over the expanded catalog with the built resolvers answers one port per
  route, with the route's protocol, and `llm_models::port` for a direct `<provider>/<model-id>` name
  builds through the same rules;
- no secret material leaves the document: for an `env:` provider whose variable is set to a marker
  value, neither `format!("{:?}")` of the loaded routes or resolvers, nor `serde_json::to_string` of
  the expanded catalog's bindings, nor any refusal's message contains the marker, the variable name
  or the file path.

## Verification

`task conformance`; `cargo test -p b10x-llm-models --all-features`; `cargo test -p b10x-llm-models`;
`cargo clippy -p b10x-llm-models --all-targets --all-features -- -D warnings`; `cargo test -p
b10x-llm-core --test dependency_boundary`. No network I/O, no paid call.

## Vocabulary change

Decided by the plan's owner on 2026-10-16, during the wave and before 0.6.0 is tagged: the
top-level `providers:` map of `llm.routes/1` is named **`endpoints:`**. A route value is
`<endpoint>/<model-id>` and a direct name `<declared endpoint>/<model-id>`. Every mention above of a
routes-document "provider" (the map, its entries, "undeclared provider", "the provider's reference")
reads as "endpoint"; `providers:` is refused as an unknown field. Reason: in the connectors vocabulary
"provider" means the company and one API is a "connector", so one word would mean two things across
the command lines; llm's own declarations already call the `base_url` holder an endpoint. The
expansion is unchanged: an endpoint's name becomes the catalog provider, account and endpoint id and
the secret reference id. Applied in correction round 2 to the spec (`llm.routes.Loading.endpoints`),
code, tests, scenarios, error texts and the routes guide.
