---
title: Not yet
sidebar_position: 2
description: What is not implemented, what blocks each item, and the order the remaining work has to happen in.
lede: Every planned item on the status page, with what it waits for.
source: docs/implementation-status.md, docs/design.md
---

# Not yet

These are not implemented. Each entry says what is missing and what it waits for. The
[Status](/docs/status) page lists the same items among everything that ships.

| Item | State | Waits for |
| --- | --- | --- |
| Provider access qualification (API and subscription) | Not qualified; one live Codex probe outside the gate | Recorded live evidence; for subscriptions, a documented supported contract per provider |
| A published contract and compatibility policy | The versioned envelopes ship; the policy is not published | The contract release |
| The gateway and hosting crates in llm-gateway | In llm today | The move to their own repository |
| Gateway translation | Not started; the gateway refuses to translate | The move to llm-gateway, and the Modal adapter |
| Modal hosting adapter | `b10x-llm-modal` exports nothing | A Modal account, a credential source and an authorised paid qualification run |
| A production Runpod transport | Only the in-process emulator exists | Its own work item; the live control-plane assumptions on [Hosting](../concepts/hosting.md#not-verified) are unchecked |
| Operator command line | `b10x-llm-cli` exports nothing | Gateway translation |
| Harness building on llm | Harness uses its own model wire crates | The Harness side of the move |
| A qualified release | Releases exist; none is qualified | All of the above |

## Blocked items

### Modal hosting

The acceptance for the Modal adapter requires a recorded live deployment, readiness check and
cleanup. That needs a Modal account, a credential and a paid run. None is available to this
repository, and the ordinary gate never makes a paid call.

### Subscription access

Calling a model through a caller's subscription, rather than a metered API key, works mechanically:
`codex_model` reads a Codex login, and `codex-renewal` renews it. It is not qualified for either
provider. Holding a token is not evidence that a given use is supported: each provider's supported
contract and permitted deployment context must be established first. The design is fixed in two
ways: the caller owns credential acquisition, and llm never runs a login flow. A rejected
subscription credential never falls back to a billable API key.

## The order of the remaining work

```mermaid
flowchart TD
  S["Gateway and hosting move<br/>to llm-gateway"] --> T["Gateway translation"]
  M["Modal adapter<br/>(blocked: account, paid run)"] --> T
  T --> C["Operator CLI"]
  A["Provider access qualification<br/>(blocked: subscription contract)"] --> Q["Qualified release"]
  C --> Q
  R["Published contract"] --> Q
```

- **Gateway translation** exposes the three protocol surfaces over the published neutral subset,
  preserving streaming, tools, cancellation and usage, or refusing explicitly.
- **The operator command line** validates, inspects and runs one configuration from the same TOML.
  Inspection must resolve no secret and start no resource.
- **The qualified release** ties one published version to the full evidence matrix, including live
  API, subscription and hosting results.

## Smaller open items

- **A Secrets resolver.** An optional adapter that resolves a `SecretRef` through the named storage
  of [Secrets](https://beyond10x.github.io/secrets/) ([GitHub](https://github.com/beyond10x/secrets)).
  It waits for that library's release.
- **Spend enforcement in effectful paths.** The spending ledger exists, but no client or gateway
  takes its permits yet. Routing's `admit` port is where a caller connects a limit today.
- **Runpod cleanup outside the ledger.** Orphan sweeps and inherited-pod terminations bypass the
  hosting controller, so no stop obligation is recorded for them. Fixing this needs a change to the
  hosting contract.

## Out of scope for this milestone

Choosing the cheapest model automatically. A resolver backed by a connector integration product,
which has been superseded by the Secrets adapter above.
