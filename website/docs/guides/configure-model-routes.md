---
title: Configure model routes
sidebar_position: 6
description: One short llm.routes/1 document, in YAML or TOML, names endpoints, routes, aliases and prices; it expands into a catalog, the Bedrock targets outside it and the resolvers its secret references name.
lede: A gateway, a command line and an SDK read one routes document the same way, and a name reaches exactly one endpoint and model.
source: crates/llm-models/src/routes.rs, crates/llm-models/tests/routes.rs, crates/llm-models/tests/routes_toml_bedrock.rs, spec/domains/routes.yaml, contracts/routes/scenarios
---

# Configure model routes

An `llm.routes/1` document is the short form of a catalog. It declares endpoints and routes. An
endpoint says where, which wire and how to authenticate. A route is a name for one
`<endpoint>/<model-id>`.

`llm-models` reads the document with `llm_models::routes::Routes::parse` and expands it into the
same `llm.catalog/1` catalog the [routing](../concepts/routing.md) concept describes. Ports are
then built by the unchanged `llm_models::port` and `CatalogModels::build`. `Routes::parse_toml`
reads the same document written in TOML, as the [llm command](use-the-llm-command.md) does. Both
forms take the same keys, refuse the same mistakes and give the same `digest()`.

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
| `auth.kind` | `anonymous`, `bearer` or `api-key`; on a Bedrock endpoint `aws-keys`, `aws-container`, `aws-web-identity` or `aws-process` |
| `auth.header` | The header an `api-key` endpoint sends; required for `api-key` and refused otherwise |
| `auth.secret` | A reference, never a value: `env:<VARIABLE>`, `file:<absolute path or ~/path>` or `keychain:<service>/<entry>`; required for `bearer` and `api-key`, refused for `anonymous` |

A credentialed endpoint is billed `metered` and an anonymous one `self-hosted`. A
`subscription-oauth` kind is refused, and the error names it. Subscription tokens stay a catalog
account.

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
  bedrock-irsa:   {protocol: bedrock-converse, bedrock: {region: eu-central-1},
                   auth: {kind: aws-web-identity}}
```

| Field | Values |
| --- | --- |
| `protocol` | `bedrock-converse` for Converse; `messages` for the Anthropic body on `InvokeModel` |
| `bedrock.region` | Required; the request is signed for it |
| `base_url` | Optional; replaces `https://bedrock-runtime.{region}.amazonaws.com` |
| `auth.kind` | `aws-keys`, `aws-container`, `aws-web-identity` or `aws-process`; exactly one credential source |
| `auth.access_key_id`, `auth.secret_access_key` | References, required for `aws-keys` |
| `auth.session_token` | A reference, optional for `aws-keys` |
| `auth.command` | A list of strings: the program (non-empty), then its arguments; required for `aws-process`, refused otherwise |

An `aws-container` endpoint signs with the credentials the container endpoint serves, as EKS Pod
Identity and ECS task roles do. The caller supplies that endpoint's URI and its token file with
`SecretSources::with_container_credentials`. Without them the endpoint gets no resolver, so
`Routes::port` refuses its routes as `unauthorized` and every other route still builds. The
endpoint's name is the provider, account and endpoint of its turns' provenance.

An `aws-web-identity` endpoint signs with the credentials STS `AssumeRoleWithWebIdentity` answers,
as IAM roles for service accounts (IRSA) use it. The document declares only the kind. The caller
passes two values to `SecretSources::with_web_identity`:

- a `WebIdentityRole`: the role ARN (`AWS_ROLE_ARN` on IRSA), a session name and its region's STS
  endpoint;
- the token file (`AWS_WEB_IDENTITY_TOKEN_FILE`).

```rust
let sources = SecretSources::new().with_web_identity(
    WebIdentityRole::new(role_arn, "my-service", "https://sts.eu-central-1.amazonaws.com"),
    token_file,
);
```

The token file is read as a platform-mounted file, the same way as the container token. It is read
at each fetch, not when the resolvers or the port are built. Any other `auth` field is refused,
the four values included, so no secret is a document value.

Without the role and the token file the endpoint gets no resolver, and `Routes::port` refuses its
routes as `unauthorized`. So does a role ARN, session name or STS endpoint that
`WebIdentityCredentials` refuses. Every other route still builds. The `llm` command supplies no
web identity role yet, so it refuses such a route.

An `aws-process` endpoint runs its `command` without a shell, as an AWS profile's
`credential_process` does, and signs with the keys it prints. The output is the
`credential_process` JSON answer: `Version` 1 with a non-empty `AccessKeyId` and
`SecretAccessKey`, optionally a `SessionToken` and an RFC 3339 `Expiration`. The keys stay in the
calling process's memory.

| When | What happens |
| --- | --- |
| The document is parsed, the catalog is built or the routes are listed | The command does not run |
| The first turn | The command runs |
| The keys are within five minutes of expiring | A turn is answered with the kept keys while one further run refreshes them |

The command's stdin is empty, its stderr is the caller's, its stdout is read up to 64 KiB, and it
is killed after 60 seconds. A non-zero exit, a timeout or an unusable answer refuses the turn as
`unauthorized`. The error names the endpoint and the program and carries none of the output,
and nothing is sent.

`Routes::port` builds an `aws-process` route only with `llm-models` feature `process`, and only
from resolvers the caller marked with `RouteResolvers::allow_credential_process`. Otherwise the
route is refused as `unauthorized`, no command runs and every other route still builds. The `llm`
command allows it.

## Routes and names

A route value splits on its **first** `/` only: `openrouter/anthropic/claude-sonnet-4.5` is endpoint
`openrouter`, model `anthropic/claude-sonnet-4.5`. A name that is not a route but reads
`<declared endpoint>/<model-id>` resolves directly, so `anthropic/claude-opus-4-1` reaches the `anthropic` endpoint
without a route entry. Any other name is refused as `invalid-request`.

A route written as a string declares the default capabilities:

| Capability | Default |
| --- | --- |
| tools, tool choice, temperature, `top_p` | allowed |
| reasoning efforts | `low`, `medium` and `high` |
| context window | 200000 tokens |
| output tokens | 16384 |
| `output_streaming` | `true` |
| `input_during_output` | `false` |

Capabilities decide which requests a route admits, and a Messages endpoint sends
`max_output_tokens` as its default `max_tokens`. A model that does not fit the default is written
as a mapping with its own `capabilities`, as in a catalog: the seven settings, plus
`output_streaming` and `input_during_output`, which may be left out.

A route claiming input during output, or output that does not stream, is refused, and the error
names the setting. No protocol here carries input during output, and every one streams its output.

`structured_output` and `image_input` may also be declared, and are not declared when left out. A
route declaring one `false` refuses a turn with an output format or a user image before I/O, and
selection over the route's catalog rejects it
([Routing](../concepts/routing.md#structured-output-and-image-input)). An undeclared route's wire
carries the turn or refuses it before I/O. No built-in default declares either.

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

`Routes::cost(name, &observation)` prices one turn by the `llm-cost` rules. The price is the
route's model's. The turn is attributed to the model its provider reported, in
`observation.upstream_model`. When that is another model or absent, every line is unknown.

A counter the turn did not report, a rate the price leaves out and usage that is not final each
leave a line unknown. The complete total exists only when no line is unknown. A route without a
price has no cost, not a cost of 0.

## Expansion

| Document | Catalog |
| --- | --- |
| endpoint `anthropic` | provider, account and endpoint with id `anthropic`; the account's secret reference id is `anthropic` |
| route `sonnet` | served model, serving model and a single-target route without fallback, each with id `sonnet` |
| direct name `anthropic/claude-opus-4-1` | the same, in a one-entry catalog built for that name |

## Credentials

`Routes::resolvers(&SecretSources)` builds one resolver per credentialed endpoint, keyed by the
endpoint's name, from the existing local adapters. Features `environment`, `file` and `keychain` of
`llm-models` are off by default and forward to the same features of `llm-credentials`.
`SecretSources::with_home` supplies the directory `file:~/` expands against. No code reads `HOME`
on its own. A `keychain:` reference needs a store injected with `SecretSources::with_keychain`.

Building the resolvers reads nothing and refuses only an over-long endpoint name. An endpoint
whose source cannot be built gets no resolver:

- a reference whose feature is off;
- a keychain reference without a store;
- a `file:~/` reference without a home;
- a container token file that is not an absolute path.

A port for one of its routes is then refused as `unauthorized`, and the error names the endpoint. The
other routes still build.

There is no ambient fallback. An unset variable or a missing file is refused when the turn
prepares its credential, before any request is sent. The refusal is `unauthorized` and cites the
endpoint's reference: ``secret reference `anthropic` refused: secret reference was not found``.
The variable name, the path and the value do not appear in an error, a `Debug` rendering or the
expanded catalog.

```rust
use llm_models::routes::{Routes, SecretSources};
use llm_models::CatalogModels;

let routes = Routes::parse(&std::fs::read_to_string("routes.yaml")?)?;
let resolvers = routes.resolvers(&SecretSources::new().with_home(home_dir))?;
let port = routes.port("sonnet", http.clone(), &resolvers)?;      // one name
let every_route = CatalogModels::build(routes.catalog(), &http, &resolvers)?; // all routes
```

## Refusals

Parsing performs no I/O and is strict. Each of these is refused as `invalid-request`. The error
names the field, endpoint or route and does not echo the refused value.

- an unknown field at any level
- a repeated key
- a `format` other than `llm.routes/1`
- an unknown protocol or authentication kind
- a secret without a known source prefix
- an `api-key` endpoint without `header`
- an `anonymous` endpoint with a `secret`
- a route to an undeclared endpoint
- a Bedrock endpoint without a Region or with two credential sources
- an alias claimed twice
- an invalid price

A malformed document is refused by its line and column, and in TOML an unknown key also by its
name. The `llm.routes` ESS domain (`spec/domains/routes.yaml`) holds these rules, and
`contracts/routes/scenarios/` checks them against the real crate on every `task check`.

A document over 2 MiB, one whose YAML aliases would expand it past its bound while it is read, and
one whose expansion exceeds the catalog's own bound (several thousand routes) are refused as
`too-large`. A direct name is one identifier of at most 256 bytes.
