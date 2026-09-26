---
title: The gateway
description: An authenticated single-owner HTTP surface that reports liveness and readiness and lets its owner read which routes it serves. It does not translate or proxy model calls yet.
---

# The gateway

`b10x-llm-gateway` is the start of a network surface for one owner or one trusted deployment. Today
it does four things: it authenticates that owner, reports liveness and readiness, lists the routes
the deployment serves, and starts, drains and stops deliberately.

:::caution It does not answer model requests yet
The crate performs no protocol translation, proxies no model call, resolves no secret and reaches
no network. Pointing a Chat Completions client at it does not produce a model answer. That is the
unbuilt gateway translation work; see [Not yet](../status/roadmap.md).
:::

## Why it cannot leak a secret

The crate has **no dependencies at all**: not the credential crate, not the HTTP client, not the
hosting crates, not an async runtime. A test checks this over the full dependency tree cargo
resolves. Listing routes therefore cannot resolve a secret or start a resource, because nothing in
the build could.

The gateway is never given a credential *source*. The embedding resolves the owner's credential
once, from whatever source it chose, and hands over an `OwnerToken`. That token is redacted in
`Debug`, has no `Display`, `Clone` or serialization, and is overwritten on drop.
`SharedSecretVerifier` compares in constant time with respect to where bytes differ, and refuses a
secret shorter than 32 bytes. An embedding with another scheme implements `OwnerVerifier` instead.

## The route inventory

A `RouteInventory` is an immutable snapshot the embedding builds: routes, their ordered targets,
each target's protocol, provider, account, endpoint, model and binding revision by identifier, the
authentication and billing *kinds*, and declared limits. There is no field for an endpoint URL, a
secret reference or credential material.

The response contains exactly the identifier bytes the embedding supplied. Passing identifiers,
not URLs, is the embedding's responsibility. A limit the snapshot did not report is omitted, never
rendered as `0`.

## The HTTP surface

| Method | Path | Credential | Answer |
| --- | --- | --- | --- |
| `GET`, `HEAD` | `/health` | none | `200 {"status":"live"}` while the process serves |
| `GET`, `HEAD` | `/ready` | none | `200 {"status":"ready"}`, or `503 {"status":"unready"}` before `mark_ready` and while draining |
| `GET`, `HEAD` | `/v1/routes` | owner | Every route in the snapshot |
| `GET`, `HEAD` | `/v1/routes/{alias}` | owner | One route by alias |

Every response is `application/json` with `cache-control: no-store` and `connection: close`: one
request per connection. Everything except the two probe paths is authenticated **before** anything
else is decoded, so an unauthenticated request to an unknown path gets `credential-absent`, not
`path-unknown`. The surface cannot be mapped without the credential. No request body is ever read.

## Refusals

A refusal names a stable code and a fixed message. It never echoes what the caller sent.

| Code | Status |
| --- | --- |
| `body-not-allowed` | 400 |
| `credential-absent` | 401 |
| `credential-malformed` | 401 |
| `credential-rejected` | 401 |
| `method-not-allowed` | 405 |
| `overloaded` | 503 |
| `path-unknown` | 404 |
| `request-malformed` | 400 |
| `request-too-large` | 431 |
| `route-unknown` | 404 |
| `unavailable` | 503 |

## Lifecycle

- `Gateway::bind` listens immediately and starts **not ready**.
- `mark_ready` opens inspection.
- `begin_drain` reports unready while still serving, so a load balancer can remove it. A drain
  cannot be undone.
- `shutdown` stops accepting, lets accepted connections finish, and returns a `ShutdownReport` of
  accepted, completed and in-flight connections.

## Bounds

| Bound | Value |
| --- | --- |
| Owner credential | at most 4096 bytes |
| Shared secret | at least 32 bytes |
| Identifier label | at most 256 bytes |
| Targets per route | at most 64 |
| Routes | at most 4096 |
| Request head | 8192 bytes by default |
| Concurrent requests | 64 by default |
| Head read timeout | 10 seconds by default |

[Start the gateway](../guides/start-the-gateway.md) composes one and queries it with `curl`.
