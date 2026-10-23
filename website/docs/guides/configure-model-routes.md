---
title: Configure model routes
sidebar_position: 6
description: One short llm.routes/1 document, in YAML or TOML, names endpoints, routes, aliases and prices; it expands into a catalog, the Bedrock targets outside it and the resolvers its secret references name.
lede: A gateway, a command line and an SDK read one routes document the same way, and a name reaches exactly one endpoint and model.
source: crates/llm-models/src/routes.rs, crates/llm-models/tests/routes.rs, crates/llm-models/tests/routes_toml_bedrock.rs, spec/domains/routes.yaml, contracts/routes/scenarios
---

# Configure model routes

An `llm.routes/1` document is the short form of a catalog. It names endpoints (where, which wire,
how to authenticate) and routes (a name for one `<endpoint>/<model-id>`). `llm-models` reads it
with `llm_models::routes::Routes::parse` and expands it into the same `llm.catalog/1` catalog the
[routing](../concepts/routing.md) concept describes, so ports are built by the unchanged
`llm_models::port` and `CatalogModels::build`. `Routes::parse_toml` reads the same document
written in TOML, as the [llm command](use-the-llm-command.md) does; both forms take the same keys,
refuse the same mistakes and give the same `digest()`.

```yaml
format: llm.routes/1
endpoints:
  anthropic:  {base_url: https://api.anthropic.com/v1, protocol: messages,
               auth: {kind: api-key, header: x-api-key, secret: env:ANTHROPIC_API_KEY}}
  openrouter: {base_url: https://openrouter.ai/api/v1, protocol: chat-completions,
               auth: {kind: bearer, secret: env:OPENROUTER_API_KEY}}
  gateway:    {base_url: https://gateway.internal/v1, protocol: responses,
               auth: {kind: bearer, secret: file:~/.config/b10x/gateway-token}}
  lab:        {base_url: http://127.0.0.1:8000/v1, protocol: chat-completions,
               auth: {kind: anonymous}}
routes:
  sonnet:    anthropic/claude-sonnet-4-5
  or-sonnet: openrouter/anthropic/claude-sonnet-4.5
  qwen:      gateway/qwen
  small:     {model: lab/small, capabilities: {tools: false, tool_choice: false,
              temperature: true, top_p: true, reasoning_efforts: [], context_window: 8192,
              max_output_tokens: 2048}}
```

## Endpoints

| Field | Values |
| --- | --- |
| `base_url` | An HTTP(S) API prefix with no userinfo, query or fragment |
| `protocol` | `messages`, `responses` or `chat-completions`; on a Bedrock endpoint `bedrock-converse` or `messages` |
| `bedrock.region` | Makes the endpoint a Bedrock endpoint; see [Bedrock endpoints](#bedrock-endpoints) |
| `auth.kind` | `anonymous`, `bearer` or `api-key`; on a Bedrock endpoint `aws-keys`, `aws-container` or `aws-process` |
| `auth.header` | The header an `api-key` endpoint sends; required for `api-key` and refused otherwise |
| `auth.secret` | A reference, never a value: `env:<VARIABLE>`, `file:<absolute path or ~/path>` or `keychain:<service>/<entry>`; required for `bearer` and `api-key`, refused for `anonymous` |

A credentialed endpoint is billed `metered` and an anonymous one `self-hosted`. A
`subscription-oauth` kind is refused by name: subscription tokens stay a catalog account.

## Bedrock endpoints

An endpoint with a `bedrock` table is an Amazon Bedrock Runtime endpoint. It has no binding, so it
stays outside the catalog, and `Routes::port` builds `llm-bedrock`'s `BedrockClient` for its routes.

```yaml
endpoints:
  bedrock-eu:     {protocol: bedrock-converse, bedrock: {region: eu-central-1},
                   auth: {kind: aws-container}}
  bedrock-invoke: {protocol: messages, bedrock: {region: eu-central-1},
                   auth: {kind: aws-keys, access_key_id: env:AWS_ACCESS_KEY_ID,
                          secret_access_key: env:AWS_SECRET_ACCESS_KEY,
                          session_token: env:AWS_SESSION_TOKEN}}
  bedrock-dev:    {protocol: bedrock-converse, bedrock: {region: eu-central-1},
                   auth: {kind: aws-process,
                          command: [aws, configure, export-credentials, --profile, dev]}}
```

| Field | Values |
| --- | --- |
| `protocol` | `bedrock-converse` for Converse; `messages` for the Anthropic body on `InvokeModel` |
| `bedrock.region` | Required; the request is signed for it |
| `base_url` | Optional; replaces `https://bedrock-runtime.{region}.amazonaws.com` |
| `auth.kind` | `aws-keys`, `aws-container` or `aws-process`; exactly one credential source |
| `auth.access_key_id`, `auth.secret_access_key` | References, required for `aws-keys` |
| `auth.session_token` | A reference, optional for `aws-keys` |
| `auth.command` | The program, then its arguments, each a non-empty program and strings; required for `aws-process` and refused otherwise |

An `aws-container` endpoint signs with the credentials the container endpoint serves (EKS Pod
Identity, ECS task roles). The caller supplies that endpoint's URI and its token file with
`SecretSources::with_container_credentials`; without them the endpoint gets no resolver, so
`Routes::port` refuses its routes as `unauthorized` and every other route still builds. The
endpoint's name is the provider, account and endpoint of its turns' provenance.

An `aws-process` endpoint runs its `command` without a shell, as an AWS profile's
`credential_process` does, and signs with the keys it prints: the `credential_process` JSON
answer, `Version` 1 with a non-empty `AccessKeyId` and `SecretAccessKey`, optionally a
`SessionToken` and an RFC 3339 `Expiration`. The keys stay in the calling process's memory. The
command runs at the first turn, not when the document is parsed, the catalog is built or the
routes are listed, and again only when the keys are within five minutes of expiring; a turn
inside that window is answered with the kept keys while one further run refreshes them. Its stdin
is empty, its stderr is the caller's, its stdout is read up to 64 KiB, and it is killed after 60
seconds. A non-zero exit, a timeout or an unusable answer refuses the turn as `unauthorized`,
naming the endpoint and the program and carrying none of the output; nothing is sent.

Running a command is the caller's decision, not the document's. `Routes::port` builds an
`aws-process` route only with `llm-models` feature `process` and only from resolvers the caller
marked with `RouteResolvers::allow_credential_process`; otherwise the route is refused as
`unauthorized`, no command runs and every other route still builds. The `llm` command allows it.

## Routes and names

A route value splits on its **first** `/` only: `openrouter/anthropic/claude-sonnet-4.5` is endpoint
`openrouter`, model `anthropic/claude-sonnet-4.5`. A name that is not a route but reads
`<declared endpoint>/<model-id>` resolves directly, so `anthropic/claude-opus-4-1` reaches the `anthropic` endpoint
without a route entry. Any other name is refused as `invalid-request`.

A route written as a string declares the default capabilities: tools, tool choice, temperature and
`top_p` allowed, reasoning efforts `low`, `medium` and `high`, a 200000-token context window,
16384 output tokens, streamed output and no input during output. Capabilities decide which requests
a route admits, and a Messages endpoint sends `max_output_tokens` as its default `max_tokens`, so a
model that does not fit the default is written as a mapping with its own `capabilities`, as in a
catalog: the seven settings, plus `output_streaming` (default `true`) and `input_during_output`
(default `false`), which may be left out. A route claiming input during output, or output that does
not stream, is refused by name: no protocol here carries input during output, and every one streams
its output.

## Aliases and prices

A route written as a mapping may also carry:

| Field | Values |
| --- | --- |
| `aliases` | Other names that reach the route. An alias names one route, is no route's name and contains no `/` |
| `price` | `currency` and per-million-token decimal strings `input`, `cached_input`, `cache_creation_input` and `output`; any rate may be left out |

```yaml
routes:
  haiku:
    model: anthropic/claude-haiku-5-5
    aliases: [fast]
    price: {currency: USD, input: "1", cached_input: "0.1", cache_creation_input: "1.25",
            output: "5"}
```

`Routes::cost(name, &observation)` prices one turn by the `llm-cost` rules. A counter the turn did
not report, a rate the price leaves out and usage that is not final each leave a line unknown, and
the complete total exists only when no line is. A route without a price has no cost, never a cost
of 0.

## Expansion

| Document | Catalog |
| --- | --- |
| endpoint `anthropic` | provider, account and endpoint with id `anthropic`; the account's secret reference id is `anthropic` |
| route `sonnet` | served model, serving model and a single-target route without fallback, each with id `sonnet` |
| direct name `anthropic/claude-opus-4-1` | the same, in a one-entry catalog built for that name |

## Credentials

`Routes::resolvers(&SecretSources)` builds one resolver per credentialed endpoint, keyed by the
endpoint's name, from the existing local adapters: features `environment`, `file` and `keychain` of
`llm-models` (all off by default) forward to the same features of `llm-credentials`.
`SecretSources::with_home` supplies the directory `file:~/` expands against; nothing reads `HOME` on
its own. A `keychain:` reference needs a store injected with `SecretSources::with_keychain`.
Building the resolvers reads nothing and refuses only an over-long endpoint name. An endpoint
whose source cannot be built gets no resolver: a reference whose feature is off, a keychain
reference without a store, a `file:~/` reference without a home, or a container token file that
is not an absolute path. A port for one of its routes is then refused as `unauthorized`, naming
the endpoint, and every other route still builds.

There is no ambient fallback. An unset variable or a missing file is refused when the turn
prepares its credential, before any request is sent, as `unauthorized` and by the endpoint's
reference: ``secret reference `anthropic` refused: secret reference was not found``. The variable
name, the path and the value never appear in an error, a `Debug` rendering or the expanded catalog.

```rust
use llm_models::routes::{Routes, SecretSources};
use llm_models::CatalogModels;

let routes = Routes::parse(&std::fs::read_to_string("routes.yaml")?)?;
let resolvers = routes.resolvers(&SecretSources::new().with_home(home_dir))?;
let port = routes.port("sonnet", http.clone(), &resolvers)?;      // one name
let every_route = CatalogModels::build(routes.catalog(), &http, &resolvers)?; // all routes
```

## Refusals

Parsing performs no I/O and is strict. Each of these is refused as `invalid-request`, naming the
field, endpoint or route and never echoing the refused value: an unknown field at any level, a
repeated key, a `format` other than `llm.routes/1`, an unknown protocol or authentication kind, a
secret without a known source prefix, an `api-key` endpoint without `header`, an `anonymous`
endpoint with a `secret`, a route to an undeclared endpoint, a Bedrock endpoint without a
Region or with two credential sources, an alias claimed twice, and an invalid price. A malformed
document is refused by its line and column, and in TOML an unknown key also by its name. The `llm.routes` ESS domain
(`spec/domains/routes.yaml`) holds these rules, and `contracts/routes/scenarios/` checks them
against the real crate on every `task check`.

A document over 2 MiB, one whose YAML aliases would expand it past its bound while it is read, and
one whose expansion exceeds the catalog's own bound (several thousand routes) are refused as
`too-large`. A direct name is one identifier of at most 256 bytes.
