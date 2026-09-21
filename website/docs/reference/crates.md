---
title: Crate layout
description: Fourteen workspace crates — eleven implemented, three declared boundaries that are stubs today.
---

# Crate layout

The workspace is `publish = false` at version `0.0.0`. Nothing here is on a registry yet.
`unsafe_code` is forbidden workspace-wide, and Clippy runs `all` plus `pedantic` at deny.

## Implemented

Six landed before the current wave:

| Crate | What it owns |
| --- | --- |
| `b10x-llm-core` | Neutral model requests, results, streaming, capabilities and typed failures. No I/O, no execution authority. |
| `b10x-llm-credentials` | Injected secret resolution and caller-managed refresh; optional backend adapters. |
| `b10x-llm-http` | Shared bounded HTTP and SSE transport. No vendor fields, no credential acquisition. |
| `b10x-llm-providers` | Provider, account and authentication bindings, independent of protocol selection. |
| `b10x-llm-routing` | TOML catalogs, capability-aware resolution and explicit ordered fallback declaration. |
| `b10x-llm-cost` | Usage accounting, versioned price estimates and explicit admission limits. |

Five more land with it:

| Crate | What it owns |
| --- | --- |
| `b10x-llm-responses` | Responses protocol projection for the declared supported subset. |
| `b10x-llm-messages` | Messages protocol projection for the declared supported subset. |
| `b10x-llm-chat` | Chat Completions projection, including arbitrary compatible endpoints. |
| `b10x-llm-gateway` | Authenticated single-owner composition: authenticate before decoding, read-only route inspection, deliberate start/drain/stop. |
| `b10x-llm-provision` | The hosting lifecycle every provider adapter is held to, plus an in-process `FakeProvider` that demonstrates it. |

Two of those five are narrower than their names suggest, deliberately:

- **`b10x-llm-gateway` does not translate.** Its own module documentation lists what it refuses:
  "protocol translation, proxying a model call, resolving a secret, reaching a network, and
  multi-tenant accounts or quotas." It admits one owner and lets that owner read which routes a
  deployment serves. Translation is a separate unbuilt story.
- **`b10x-llm-provision` reaches no cloud.** It "opens no socket, reads no credential and
  allocates no cloud resource" and has no dependencies at all. It is the contract; the adapters
  that would satisfy it against a real provider do not exist.

### Optional features

| Crate | Feature | Adds |
| --- | --- | --- |
| `b10x-llm-credentials` | `file` | Explicit protected-file resolution (Linux) |
| `b10x-llm-credentials` | `keychain` | An injected `keyring_core::CredentialStore` |
| `b10x-llm-credentials` | `native-keychain` | Native store constructors; includes `keychain` |
| `b10x-llm-cost` | `sqlite` | `SqliteLedger`, the durable single-owner spending journal |

Every one of these is off by default, and the gate proves it by building each crate with
`--no-default-features`.

## Declared, not implemented

Three crates remain boundaries so that the seams are fixed before the code arrives. Each is a
five-line stub.

| Crate | Intended boundary |
| --- | --- |
| `b10x-llm-runpod` | Runpod provisioning adapter for vLLM |
| `b10x-llm-modal` | Modal provisioning adapter |
| `b10x-llm-cli` | Operator configuration, route inspection and gateway commands |

Also in the workspace: `checks/conformance`, the `b10x-llm-conformance` ESS adapter and runner.

## Dependency direction

Core depends on no consumer repository, and no crate here depends on the Harness agent loop, the
archived Platform inference component or `llmgw`. Those are *source provenance* for extracted
behaviour, never retained dependencies. Core must never depend on Connectors either: a future
arbitrary-secret adapter implements `SecretResolver` without any route reference changing.
