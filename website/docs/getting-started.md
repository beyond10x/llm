---
title: Getting started
sidebar_position: 2
description: Build the workspace, run the examples that need no provider account, and depend on a crate from your own project.
lede: Everything on this page runs offline, sends no request to a provider and reads no credential you already have.
source: crates/*/examples, crates/llm-docs/examples, Cargo.toml, .github/workflows/gate.yml
---

# Getting started

## Prerequisites

| Tool | Version | Needed for |
| --- | --- | --- |
| Rust | 1.98, edition 2024 | The workspace |
| [Task](https://taskfile.dev) | v3 | `task check` and `task rust` |
| [ESS](https://beyond10x.github.io/ess/) ([GitHub](https://github.com/beyond10x/ess)) | 0.36.0 in CI | The specification and conformance stages of `task check` |
| [AEP](https://beyond10x.github.io/docs/aep/) ([GitHub](https://github.com/beyond10x/aep)) | 0.68.0 in CI | The planning stage of `task check` |

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

## Force one tool call

```bash
cargo run --locked -p llm-docs --example forced_tool_call
```

```text
{"component":"importer","severity":"high"}
```

`call_tool` forced one tool on a recorded model and returned the call's arguments.
[Call a model with one forced tool](guides/call-a-model-with-one-forced-tool.md) shows the program
and how to point it at a Codex login instead.

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

## Depend on a crate from your own project

Nothing is published to crates.io; every package is `publish = false`. Depend on a crate from Git
at a [release tag](https://github.com/beyond10x/llm/releases), as Loom does:

```toml
[dependencies]
llm-core = { package = "b10x-llm-core", git = "https://github.com/beyond10x/llm", tag = "<release>" }
llm-responses = { package = "b10x-llm-responses", git = "https://github.com/beyond10x/llm", tag = "<release>" }
```

Package names start with `b10x-`; library names do not, so the code says `use llm_core::…`.
[Crates](reference/crates.md) lists every package, its library name and its features. The source
is licensed under Apache-2.0 from 0.1.2 on; see
[`LICENSE`](https://github.com/beyond10x/llm/blob/main/LICENSE).

## Next

- [Call a local endpoint](guides/call-a-local-endpoint.md): send a real turn to a vLLM-compatible
  server declared in TOML.
- [Run the checks](guides/run-the-checks.md): what `task check` proves, and what it does not.
