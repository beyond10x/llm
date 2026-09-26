---
title: Crate layout
description: Fourteen workspace crates. Twelve are implemented, two are empty placeholders.
---

# Crate layout

The workspace is `publish = false` at version `0.0.0`. Nothing is on a registry. Package names
start with `b10x-`; library names do not (`b10x-llm-core` is `use llm_core`). `unsafe_code` is
forbidden workspace-wide, and Clippy runs `all` plus `pedantic` at deny.

## Implemented

| Crate | What it owns | Performs I/O |
| --- | --- | --- |
| `b10x-llm-core` | The neutral turn: `Model`, requests, items, stream events, outcomes, observations, capabilities and typed failures | No |
| `b10x-llm-credentials` | `SecretRef`, the injected `SecretResolver`, coordinated renewal, and optional file and keychain adapters | Only the optional adapters |
| `b10x-llm-http` | Bounded single-attempt HTTP and SSE transport: deadlines, no redirects, no automatic retry | Yes |
| `b10x-llm-providers` | Bindings: provider, account, endpoint, model and serving declaration, and request-time authentication headers | Only through the injected resolver |
| `b10x-llm-routing` | `llm.catalog/1` TOML catalogs, selection, explanation and ordered fallback over caller-supplied models | No; the models it runs do |
| `b10x-llm-cost` | Price books, usage pricing, and the optional SQLite spending ledger | Only the `sqlite` feature |
| `b10x-llm-chat` | Chat Completions projection, `ChatClient` and ingress codec | `ChatClient` |
| `b10x-llm-messages` | Messages projection, `MessagesClient` and ingress codec | `MessagesClient` |
| `b10x-llm-responses` | Responses projection and ingress codec | No |
| `b10x-llm-gateway` | Authenticated single-owner HTTP surface: probes and read-only route inventory | Yes; no dependencies at all |
| `b10x-llm-provision` | The hosting lifecycle contract and an in-process `FakeProvider` | No; no dependencies at all |
| `b10x-llm-runpod` | Runpod vLLM adapter behind the hosting contract, with the in-process `EmulatedRunpod` | No production transport exists |

Also in the workspace: `checks/conformance`, the `b10x-llm-conformance` ESS adapter and runner.
It is verification tooling, not a library to depend on.

### Which crate do I need?

| To… | Depend on |
| --- | --- |
| Write a caller that works with any model | `b10x-llm-core` |
| Call a Chat Completions or vLLM endpoint | `b10x-llm-chat`, `b10x-llm-http`, `b10x-llm-credentials`, and a binding from `b10x-llm-routing` or `b10x-llm-providers` |
| Call a Messages endpoint | `b10x-llm-messages` and the same three |
| Build or read Responses bodies yourself | `b10x-llm-responses` |
| Declare routes in TOML and fall back between targets | `b10x-llm-routing` |
| Price usage, or limit spend | `b10x-llm-cost`, with `sqlite` for the ledger |

### Optional features

| Crate | Feature | Adds |
| --- | --- | --- |
| `b10x-llm-credentials` | `file` | Explicit protected-file resolution (Linux) |
| `b10x-llm-credentials` | `keychain` | An injected `keyring_core::CredentialStore` |
| `b10x-llm-credentials` | `native-keychain` | Native store constructors; includes `keychain` |
| `b10x-llm-cost` | `sqlite` | `SqliteLedger`, the durable single-owner spending journal |

All are off by default, and the gate builds each crate with `--no-default-features` to prove it.

## Placeholders

| Crate | Intended boundary | State |
| --- | --- | --- |
| `b10x-llm-modal` | Modal hosting adapter | Exports nothing |
| `b10x-llm-cli` | Operator command line: validate, inspect, run one configuration | Exports nothing |

Each exists so that the dependency seams are fixed before the code arrives.
[Not yet](../status/roadmap.md) says what blocks them.

## Dependency direction

Core depends on no consumer. No crate here depends on an agent loop or on the gateway it is meant
to replace; code ported from those projects is attributed as source, never kept as a dependency.
Core will never depend on a secret-storage product either: a new storage backend is one more
`SecretResolver`, and no route reference changes.
