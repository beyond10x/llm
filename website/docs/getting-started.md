---
title: Getting started
description: Build the workspace and run the three examples that need no provider account.
---

# Getting started

Everything on this page runs offline. No example makes a paid provider call, provisions a resource
or reads an existing user credential.

## Prerequisites

| Tool | Version | Needed for |
| --- | --- | --- |
| Rust | 1.98, edition 2024 | The workspace |
| [ESS](https://beyond10x.github.io/docs/ess/) | 0.26.0 | `task check`'s specification validation |
| [AEP](https://beyond10x.github.io/docs/aep/) | 0.55.0 | `task check`'s planning validation |

## Get the source

```bash
git clone https://github.com/beyond10x/llm.git
cd llm
cargo test --workspace --locked
```

The workspace forbids `unsafe`, and Clippy runs `all` plus `pedantic` at deny. There is no
published crate to depend on: the workspace is `publish = false` at version `0.0.0`.

## Run a model turn with no gateway and no credentials

```bash
cargo run --locked -p b10x-llm-core --example embedded
```

The example completes a turn against an in-process model, which is what the neutral port is for:
the caller depends on `Model::turn`, not on a vendor SDK.

## Explain a route without resolving a secret

```bash
cargo run --locked -p b10x-llm-routing --example explain
```

It reads [`examples/catalog.toml`](https://github.com/beyond10x/llm/blob/main/examples/catalog.toml)
and prints the selected target, every candidate's provenance and capabilities, and each candidate's
refusal reasons. It resolves no secret, sends no request and performs no I/O beyond reading the
file. [Explain a route](guides/explain-a-route.md) walks through the output.

## Quote a price for recorded usage

```bash
cargo run --locked -p b10x-llm-cost --example quote
```

It prices fixture observations against a fictional price book and prints an estimate of
`0.00022 USD`. [Price recorded usage](guides/price-recorded-usage.md) explains the bases it
separates and why there is deliberately no grand total.

## Run the full gate

```bash
task check
```

[Run the checks](guides/run-the-checks.md) describes what each stage proves.
