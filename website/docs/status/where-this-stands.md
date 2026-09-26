---
title: What works today
description: What is built, what evidence stands behind each part, and what that evidence does not establish.
---

# What works today

Every library below is implemented and tested. None of it is released, and none of it has been run
against a live provider or a real hosting account. The tests use pinned response bytes, loopback
sockets and in-process fakes, and make no paid call.

## By capability

| Capability | Crates | Evidence |
| --- | --- | --- |
| Neutral turn: port, items, streaming, cancellation, observations, typed failures | `llm-core` | ESS suite and recorded mutations |
| Bounded HTTP/SSE transport | `llm-http` | Rust fixture tests; one recorded mutation (the turn deadline) |
| Injected secrets, coordinated renewal, file and keychain adapters | `llm-credentials` | ESS suite and recorded mutations; keychain against mock stores only |
| Bindings and request-time authentication | `llm-providers` | Covered through the routing suite and recorded mutations |
| TOML catalog, selection and explanation | `llm-routing` | ESS suite and recorded mutations |
| Ordered fallback | `llm-routing` | Rust tests with scripted models, ESS scenarios in the routing suite, recorded mutations |
| Chat Completions projection and client | `llm-chat` | ESS suite and recorded mutations |
| Messages projection and client | `llm-messages` | ESS suite and recorded mutations |
| Responses projection | `llm-responses` | ESS suite and recorded mutations |
| Usage pricing | `llm-cost` | ESS suite and recorded mutations |
| Durable spending limits (SQLite) | `llm-cost` | ESS suite over real local storage, recorded mutations |
| Authenticated single-owner gateway: probes and route inventory | `llm-gateway` | Rust tests over a real loopback socket, recorded mutations; no ESS suite |
| Hosting lifecycle contract | `llm-provision` | ESS suite against `FakeProvider`, recorded mutations |
| Runpod vLLM adapter | `llm-runpod` | 52 Rust tests against `EmulatedRunpod`, 39 recorded mutations; no ESS suite |

## What the evidence means

**An ESS suite** is a set of written scenarios in the repository's
[ESS](https://beyond10x.github.io/docs/ess/) specification, run through an adapter against the real
public library functions, three times, with identical counts. A skipped scenario fails the gate.

**A recorded mutation** is a deliberate break in the production source, a named test or scenario
shown to fail because of it, and a byte-for-byte restore. Each crate's set lives in
`docs/verification/*-falsification.json`. It shows that the tests would catch a regression. It says
nothing about what a real provider returns.

These are different strengths. The gateway and Runpod adapter have recorded mutations over their
own Rust tests but no ESS suite yet; `llm-http` has its fixture tests and a single mutation.

## What no test here establishes

- That any provider's live API or subscription answers as the fixtures do.
- That the Runpod control plane behaves as the emulator does, or that stopping a pod stops billing.
- That an OS keychain service is available on a given machine.
- That a provider's usage report is correct. Pricing trusts the observations it is given.

See [Limitations](limitations.md) for the full trust boundary and [Not yet](roadmap.md) for what is
missing.

## Planning state

Lifecycle state lives in the repository's AEP planning store. Of 31 stories, 18 are implemented,
one (`runtime-contracts`: releasing the versioned contract) is active, and 12 are drafts. The drafts
are the items on [Not yet](roadmap.md), plus four stories that add ESS specifications for the
gateway, HTTP transport, provider bindings and Runpod adapter.
