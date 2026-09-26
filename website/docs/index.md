---
slug: /
title: Composable model inference
description: A Rust library for calling language models through one neutral interface, with injected credentials, explained TOML routing, ordered fallback, and usage accounting that never turns an unknown into a zero.
---

# LLM

LLM is a Rust workspace for **calling language models**. It is not an agent framework. It gives a
caller one neutral interface, a *turn*, and keeps everything that varies between vendors and
deployments behind it: the wire protocol, the account, the credential, the routing choice, the
price and the machine a model runs on.

An agent loop, a service or a command-line tool depends on `Model::turn`. Behind that port an
operator can put a self-hosted vLLM server, a metered API account or a subscription, and change it
without touching the caller.

:::caution Experimental, unreleased, and not yet tried against a live provider
The libraries are implemented and tested against fixtures, loopback sockets and in-process fakes.
**No live provider credential has been used in this repository**, no paid call has been made and
no GPU has been started. Nothing is on a registry, and there is no release. Read
[What works today](status/where-this-stands.md) before you build on it.
:::

## Why it exists

Model clients tend to fail quietly: they guess where they should have refused. LLM is built around
refusing those guesses.

| A common failure | What LLM does instead |
| --- | --- |
| A missing usage counter is billed as zero | Every usage field is optional on its own. Unknown stays unknown |
| The configured model name is reported as the one that answered | The model the endpoint reported is a separate, optional observation. A route alias never fills the gap |
| A request that failed after it was sent is retried as if it were free | Every failure says whether the request was `not-sent`, `rejected`, `accepted` or `unknown`. `unknown` is never retried elsewhere |
| Provider state from one model is replayed against another | Opaque state carries six coordinates (protocol, provider, account, endpoint, model, binding revision). All six must match before it is sent |
| A rejected subscription credential falls back to a billable key | Billing kind is independent of protocol and credential, and nothing changes it on rejection |
| An error message leaks a header or the upstream response body | Diagnostics use fixed codes and messages. They never contain headers, request bodies or upstream text |

## What you can do with it today

- **Run a turn against any Chat Completions or Messages endpoint** you declare, including an
  anonymous local vLLM server. [Call a local endpoint](guides/call-a-local-endpoint.md)
- **Declare routes in TOML** and ask which target a request would get, and why every other target
  was rejected. No secret is resolved to answer. [Explain a route](guides/explain-a-route.md)
- **Fall back in order** across the alternatives a route names, only when the failure left nothing
  visible and the request provably was not accepted. [Routing](concepts/routing.md)
- **Resolve credentials at request time** from a resolver you inject, or from a protected file or
  an OS keychain entry. [Resolve a local secret](guides/resolve-a-local-secret.md)
- **Price recorded usage** in exact decimals, and enforce a spending limit in a durable SQLite
  ledger. [Price recorded usage](guides/price-recorded-usage.md)
- **Run an authenticated single-owner gateway** that tells its owner which routes it serves.
  [Start the gateway](guides/start-the-gateway.md)
- **Drive the hosting lifecycle** that a GPU adapter is held to, and the Runpod adapter built on it,
  against an in-process emulator. [Hosting](concepts/hosting.md)

## What it does not do

LLM never executes a tool, runs a login flow, writes a credential file, or reads a vendor's
configuration directory. The gateway does not yet translate or proxy model calls. There is no
Modal adapter and no operator command line. [Not yet](status/roadmap.md) lists every gap and what
blocks it.

## Where to go next

1. [Getting started](getting-started.md): build the workspace and run the offline examples.
2. [The five concepts](concepts/overview.md): the distinctions the whole design rests on.
3. [Crate layout](reference/crates.md): which crate to depend on for what.
