---
title: Where this stands
description: The story-by-story implementation record — what is built, what is only tested against fixtures, and what is not built at all.
---

# Where this stands

The goal is the full agreed foundation and subsequent consumer adoption, not a compiling
workspace. The planning store holds twenty-four stories; **thirteen are implemented** — eight
before the current wave and the five it lands. There is no release and no published artifact, and
nothing here has been run against a live provider.

The planning store under `.engineering/planning/` owns lifecycle state; this page explains what
those states mean for a caller.

## Implemented and tested

Eight before the current wave:

| Story | What it delivers |
| --- | --- |
| `neutral-inference` | Public async port, bounded data, tool round trip, cancellation and an embedding example |
| `http-streaming` | Bounded single-attempt HTTP/SSE, terminal truth, cancellation, deadline and retry-hint fixtures |
| `secret-resolver` | Injected arbitrary secret references, redacted and zeroized material, coordinated caller-owned renewal |
| `local-secret-adapters` | Explicit file and keychain adapters; Linux file protections, exact mock-store lookup, rotation and fixed errors |
| `provider-accounts` | Validated bindings, arbitrary endpoint URLs, selected-reference request-time auth |
| `catalog-routing` | Strict versioned TOML, deterministic identity, safe explanation, ordered selection, capability admission |
| `usage-pricing` | Versioned price books, exact amounts, cache and compute pricing, attributed unknowns, failed attempts, separate recorded and subscription charges |
| `spending-limits` | Single-owner policy, pure engine, SQLite journal, concurrent admission, one-shot starts, restart uncertainty, overrun retention, compute stop obligations |

Five landing with it:

| Story | What it delivers |
| --- | --- |
| `responses-projection` | Responses request, output and streaming projections for the declared subset |
| `messages-projection` | Messages request, output and streaming projections for the declared subset |
| `chat-projection` | Chat Completions projections, including arbitrary compatible endpoints |
| `gateway-auth` | One authenticated owner, nothing decoded past the HTTP head before acceptance, a read-only route inventory carrying no endpoint URL or secret reference, and deliberate start, drain and stop |
| `hosting-contract` | The owned-resource lifecycle every provider adapter is held to — incarnation-qualified identity, requested kept separate from observed, and a stop obligation only evidence discharges — with an in-process `FakeProvider` that demonstrates it |

## Pending

| Story | What is missing |
| --- | --- |
| `runtime-contracts` | Turn v2 / outcome v3, usage and cost v2, binding and catalog v1 are implemented with ESS verification, but the story is `active`: the release and common Gates setup remain |
| `openai-access` | API and caller-managed subscription presentation, and successful qualification |
| `anthropic-access` | API and caller-managed subscription presentation, and successful qualification |
| `ordered-fallback` | Ordered alternatives, attempt accounting, refusal after exposed output or uncertain acceptance |
| `runpod-hosting` | Port and qualify Runpod vLLM deployment mechanics against the hosting contract; the crate is still a five-line stub |
| `modal-hosting` | Implement and qualify the supported Modal lifecycle operations; the crate is still a five-line stub |
| `gateway-translation` | Three ingress protocols and the translation itself. The gateway crate's own documentation lists protocol translation among the things it refuses to do |
| `operator-cli` | Validate, inspect and run one configuration, without resolving secrets or provisioning; `llm-cli` is still a five-line stub |
| `foundation-qualified` | An exact release, required checks and artifacts, and all qualification evidence |

Two further stories complete the twenty-four. `public-surface` — the manifest, this site, the
changelog, the licence and the shared Gates caller — is `active`. And
`connectors-secret-resolver` is explicitly **deferred** until
[Connectors](https://beyond10x.github.io/docs/connectors/) supports arbitrary secret custody. A
`SecretRef` encodes no backend, so that adapter must never require editing route references or
adding a Connectors dependency to core.

## What is genuinely proven

For most, but **not all**, of the implemented stories the evidence is an executable
[ESS](https://beyond10x.github.io/docs/ess/) conformance suite run against the real public library
functions three times with identical counts, plus recorded falsification: a deliberate mutation of
the production source, a named scenario shown to fail, and a byte-for-byte restore. The mutation
table for each domain lives in `docs/verification/*-falsification.json`.

Nine suites exist, one per observing domain. `gateway-auth` is the one implemented story with a
falsification record but **no ESS suite** — its evidence is that record over the crate's own Rust
tests. Read the two forms of evidence as different strengths, not as one.

Ten of the eleven implemented crates are covered by a recorded mutation:

| Crate | Falsification record |
| --- | --- |
| `llm-core` | `observations-falsification.json` |
| `llm-credentials` | `local-secrets-falsification.json` |
| `llm-providers`, `llm-routing` | `routing-falsification.json` |
| `llm-cost` | `pricing-falsification.json`, `budget-falsification.json`, `observations-falsification.json` |
| `llm-responses` | `responses-falsification.json` |
| `llm-messages` | `messages-falsification.json` |
| `llm-chat` | `chat-falsification.json` |
| `llm-gateway` | `gateway-falsification.json` |
| `llm-provision` | `hosting-falsification.json` |
| `llm-http` | **none** |

`http-streaming` is the one exception, and it is worth naming rather than hiding: no
falsification record mutates `crates/llm-http`, and that crate is not a dependency of
`checks/conformance` at all. Its evidence is its own Rust fixture tests for terminal truth,
cancellation, deadlines and retry hints — which is what `docs/implementation-status.md` claims
for it, and no more. Read its row as tested, not as falsified.

Falsification is also not qualification. Every scenario in every suite runs against fixtures,
local sockets and in-process fakes; no live provider credential has been used anywhere in this
repository. A mutation sweep proves the suite would catch a regression. It says nothing about
what a vendor returns.

Remaining work stated here is a remaining requirement. A missing qualification is not a successful
negative test.

## Completion

The foundation is complete only when every required story has its acceptance evidence, including
successful API, subscription and hosting qualification. Harness and Metaharness adopt a released
contract afterwards; `llmgw` keeps running until a reversible gateway cutover. None of those
migrations has happened.
