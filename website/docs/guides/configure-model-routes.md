---
title: Configure model routes
sidebar_position: 6
description: One short llm.routes/1 YAML document names endpoints and routes; it expands into a catalog and the resolvers its secret references name.
lede: A gateway, a command line and an SDK read one routes document the same way, and a name reaches exactly one endpoint and model.
source: crates/llm-models/src/routes.rs, crates/llm-models/tests/routes.rs, spec/domains/routes.yaml, contracts/routes/scenarios
---

# Configure model routes

An `llm.routes/1` document is the short form of a catalog. It names endpoints (where, which wire,
how to authenticate) and routes (a name for one `<endpoint>/<model-id>`). `b10x-llm-models` reads it
with `llm_models::routes::Routes::parse` and expands it into the same `llm.catalog/1` catalog the
[routing](../concepts/routing.md) concept describes, so ports are built by the unchanged
`llm_models::port` and `CatalogModels::build`.

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
| `protocol` | `messages`, `responses` or `chat-completions` |
| `auth.kind` | `anonymous`, `bearer` or `api-key` |
| `auth.header` | The header an `api-key` endpoint sends; required for `api-key` and refused otherwise |
| `auth.secret` | A reference, never a value: `env:<VARIABLE>`, `file:<absolute path or ~/path>` or `keychain:<service>/<entry>`; required for `bearer` and `api-key`, refused for `anonymous` |

A credentialed endpoint is billed `metered` and an anonymous one `self-hosted`. A
`subscription-oauth` kind is refused by name: subscription tokens stay a catalog account.

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

## Expansion

| Document | Catalog |
| --- | --- |
| endpoint `anthropic` | provider, account and endpoint with id `anthropic`; the account's secret reference id is `anthropic` |
| route `sonnet` | served model, serving model and a single-target route without fallback, each with id `sonnet` |
| direct name `anthropic/claude-opus-4-1` | the same, in a one-entry catalog built for that name |

## Credentials

`Routes::resolvers(&SecretSources)` builds one resolver per credentialed endpoint, keyed by the
endpoint's name, from the existing local adapters: features `environment`, `file` and `keychain` of
`b10x-llm-models` (all off by default) forward to the same features of `b10x-llm-credentials`.
`SecretSources::with_home` supplies the directory `file:~/` expands against; nothing reads `HOME` on
its own. A `keychain:` reference needs a store injected with `SecretSources::with_keychain`. A
reference whose feature is off, or a keychain reference without a store, is refused when the
resolvers are built. Building them reads nothing.

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
endpoint with a `secret`, and a route to an undeclared endpoint. The `llm.routes` ESS domain
(`spec/domains/routes.yaml`) holds these rules, and `contracts/routes/scenarios/` checks them
against the real crate on every `task check`.

A document over 2 MiB, one whose YAML aliases would expand it past its bound while it is read, and
one whose expansion exceeds the catalog's own bound (several thousand routes) are refused as
`too-large`. A direct name is one identifier of at most 256 bytes.
