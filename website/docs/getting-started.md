---
title: Getting started
description: Build the workspace, run the examples that need no provider account, and depend on a crate from your own project.
---

# Getting started

Everything here runs offline. No example makes a paid provider call, starts a resource or
reads a credential you already have.

## Prerequisites

| Tool | Version | Needed for |
| --- | --- | --- |
| Rust | 1.98, edition 2024 | The workspace |
| [go-task](https://taskfile.dev) | any v3 | `task check` and `task rust` |
| [ESS](https://beyond10x.github.io/docs/ess/) | 0.36.0 | The specification and conformance stages of `task check` |
| [AEP](https://beyond10x.github.io/docs/aep/) | 0.61.1 | The planning stage of `task check` |

Rust alone is enough to build, test and run the examples.

## Get the source

```bash
git clone https://github.com/beyond10x/llm.git
cd llm
cargo test --workspace --locked
```

The workspace forbids `unsafe`, and Clippy runs `all` plus `pedantic` at deny.

## Run a model turn with no network and no credential

```bash
cargo run --locked -p b10x-llm-core --example embedded
```

```text
An embedded model turn. (EndTurn; usage None)
```

The example implements `Model` in-process and calls it through the same port a real client
implements. `usage None` is not a mistake: the example reports no counters, so none are shown.
[Run a local turn](guides/run-a-local-turn.md) walks through it.

## Explain a route without resolving a secret

```bash
cargo run --locked -p b10x-llm-routing --example explain
```

It reads [`examples/catalog.toml`](https://github.com/beyond10x/llm/blob/main/examples/catalog.toml)
and prints the selected target, each candidate's provenance and capabilities, and why each other
candidate was rejected. [Explain a route](guides/explain-a-route.md) shows the output.

## Price recorded usage

```bash
cargo run --locked -p b10x-llm-cost --example quote
```

It prices fixture observations against a fictional price book and prints an `llm.cost/2` report
whose metered estimate is `0.00022 USD`. [Price recorded usage](guides/price-recorded-usage.md)
explains the report.

## Evaluate a crate from your own project

:::warning Read the licence first
The source is published under a proprietary notice, not an open-source licence.
[`LICENSE`](https://github.com/beyond10x/llm/blob/main/LICENSE) permits reading the source and
building and running it locally to evaluate, review or verify it. Any other use needs a separate
written agreement.
:::

Nothing is published to crates.io. The workspace is `publish = false` at version `0.1.3`. To try a
crate from a scratch project, depend on it from Git and pin an exact revision, because nothing
about the API is stable yet:

```toml
[dependencies]
llm-core = { package = "b10x-llm-core", git = "https://github.com/beyond10x/llm", rev = "<commit>" }
llm-chat = { package = "b10x-llm-chat", git = "https://github.com/beyond10x/llm", rev = "<commit>" }
```

Package names start with `b10x-`. Library names do not, so the code says `use llm_core::…`.
[Crate layout](reference/crates.md) says which crate owns what.

## Next

- [Call a local endpoint](guides/call-a-local-endpoint.md): send a real turn to a vLLM-compatible
  server declared in TOML.
- [Run the checks](guides/run-the-checks.md): what `task check` proves, and what it does not.
