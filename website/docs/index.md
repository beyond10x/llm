---
slug: /
title: Overview
sidebar_label: Overview
sidebar_position: 1
description: What llm is, what it is not, and where it sits among its neighbours.
lede: llm is a set of Rust libraries for calling language models through one neutral turn, with credentials, routes, retries and costs kept explicit.
source: crates/ in the llm repository at 0.1.7, CHANGELOG.md
---

# LLM

llm is a Rust workspace for **calling language models**. It gives a caller one neutral interface,
a *turn*, and keeps everything that varies between vendors and deployments behind it: the wire
protocol, the account, the credential, the route, the retry, the price and the machine a model
runs on.

An agent loop, a service or a command-line tool depends on `Model::turn`. Behind that port an
operator can put a self-hosted vLLM server, a metered API account or a subscription login, and
change it without touching the caller.

:::caution[Libraries, tested against fixtures]
Everything here is tested against recorded response bytes, loopback sockets and in-process fakes.
The gate makes no paid provider call and starts no GPU. No provider route is qualified yet; read
[Status](/docs/status) and [Limitations](status/limitations.md) before you build on it.
:::

## What it is not

- **Not an agent loop.** llm never executes a tool and holds no approval. A tool definition is a
  name, a description and a JSON Schema; when the model calls a tool, the caller runs it.
- **Not a credential store.** It resolves credentials through a resolver the embedding injects; it
  never runs a login flow, and reads a vendor's login file only where the caller points it. The one write it
  can make, renewing a Codex login, is opt-in.
- **Not a gateway service.** The gateway and hosting crates in this repository are moving to their
  own repository, llm-gateway. llm keeps the client side.

## Where it sits

| Neighbour | Relation |
| --- | --- |
| [Loom](https://beyond10x.github.io/loom/) ([GitHub](https://github.com/beyond10x/loom)) | Consumes llm: Loom's crates depend on llm's client crates at a release tag. |
| [Harness](https://beyond10x.github.io/docs/harness/) ([GitHub](https://github.com/beyond10x/harness)) | May build on llm in place of its own model wire crates; that move is planned. |
| llm-gateway ([GitHub](https://github.com/beyond10x/llm-gateway)) | Serves llm's protocols to clients: the gateway and hosting crates move there. It has no public documentation yet. |
| [ESS](https://beyond10x.github.io/ess/) ([GitHub](https://github.com/beyond10x/ess)) | Specifies llm: every library crate has an ESS domain, run by the conformance suite. |
| [Secrets](https://beyond10x.github.io/secrets/) ([GitHub](https://github.com/beyond10x/secrets)) | A planned credential source: an optional resolver over its named storage. |

## Why it exists

Model clients tend to fail quietly: they guess where they should have refused. llm is built
around refusing those guesses.

| A common failure | What llm does instead |
| --- | --- |
| A missing usage counter is billed as zero | Every usage field is optional on its own. Unknown stays unknown |
| The configured model name is reported as the one that answered | The model the endpoint reported is a separate, optional observation. A route alias never fills the gap |
| A failed request is replayed elsewhere as if it were free | Every failure says whether the request was `not-sent`, `rejected`, `accepted` or `unknown`, and whether its class may be retried. A possibly billed attempt stays in the record |
| Provider state from one model is replayed against another | Opaque state carries six coordinates (protocol, provider, account, endpoint, model, binding revision). All six must match before it is sent |
| A rejected subscription credential falls back to a billable key | Billing kind is independent of protocol and credential, and an `unauthorized` failure never falls back |
| An error message leaks a header or the upstream response body | Diagnostics use fixed codes and messages, never headers, request bodies or upstream text |

## What you can do with it today

- **Run a turn against any Responses, Messages or Chat Completions endpoint** you declare,
  including an anonymous local vLLM server. [Call a local endpoint](guides/call-a-local-endpoint.md)
- **Force one tool call** on any model and get its JSON arguments back, through a Codex login or
  any other `Model`. [Call a model with one forced tool](guides/call-a-model-with-one-forced-tool.md)
- **Run turns from a synchronous loop**, on several threads at once.
  [Use llm from a synchronous loop](guides/use-llm-from-a-synchronous-loop.md)
- **Declare routes in TOML** and ask which target a request would get and why every other target
  was rejected; no secret is resolved to answer. [Explain a route](guides/explain-a-route.md)
- **Retry and fall back**: a retriable failure that showed nothing is retried on the same target,
  then the run moves to the next named target. [Routing](concepts/routing.md)
- **Resolve credentials at request time** from a resolver you inject, a protected file, a keychain
  entry, an environment variable, a JSON document or a Codex login.
  [Resolve a local secret](guides/resolve-a-local-secret.md)
- **Price recorded usage** in exact decimals, and enforce a spending limit in a durable SQLite
  ledger. [Price recorded usage](guides/price-recorded-usage.md)

## Where to go next

1. [Getting started](getting-started.md): build the workspace and run the offline examples.
2. [The five concepts](concepts/overview.md): the distinctions the whole design rests on.
3. [Crates](reference/crates.md): every package and its features.
