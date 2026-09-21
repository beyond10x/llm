---
title: Run the checks
description: What each stage of the repository gate proves, and what it deliberately does not.
---

# Run the checks

```bash
task check
```

It runs four stages. None of them makes a paid provider call or provisions an external resource.

## 1. Rust

```bash
cargo test --workspace --locked
cargo test -p b10x-llm-credentials --all-features --locked
cargo check -p b10x-llm-credentials --no-default-features --locked
cargo test -p b10x-llm-cost --all-features --locked
cargo check -p b10x-llm-cost --no-default-features --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

The two `--no-default-features` checks matter: they prove the optional file, keychain and SQLite
dependencies really are optional, so a consumer that wants none of them pays for none of them.

`task rust` runs this stage alone, without requiring the planning tools.

## 2. Specification

```bash
ess specify validate --path spec
```

The [ESS](https://beyond10x.github.io/docs/ess/) domains under `spec/domains/` give the
repository's nouns typed homes. There are ten: `accounting`, `budget`, `catalog`, `chat`, `hosting`, `inference`, `messages`,
`responses`, `routing` and `secrets`.

Nine of the ten have an authored scenario suite under `contracts/`. The exception is `catalog`,
which describes declaration records rather than observations. One naming trap is worth knowing
before you go looking: the suite directory is `contracts/pricing/`, but the domain it exercises
is `accounting`, whose file is `spec/domains/accounting.yaml`. **There is no `pricing` domain.**

Every domain other than `catalog` describes adapter **observations of real library calls**. None
of them is a claim that this repository publishes runtime events or runs a persistent job.

## 3. Conformance

```bash
cargo run --locked -p b10x-llm-conformance -- check
```

The conformance runner checks generated-schema drift, then executes the authored scenarios against
the real public library functions three times. Its report gate refuses missing coverage, failures,
errors, unsupported observations and skips — a skipped scenario is a red verdict, not a caveat.

The adapter calls the real functions and exposes what they returned. It never reimplements the
logic under test, never reads the suite and never branches on a scenario name.

### Falsification is part of the evidence

For each asserted behaviour, the production source is deliberately broken, a **named** scenario is
shown to fail, and the source is restored byte for byte. Which mutation killed which scenario is
recorded in `docs/verification/*-falsification.json`. A suite that still passes against a broken
implementation is testing nothing, so this is checked rather than asserted.

## 4. Plan

```bash
aep plan artifact validate
```

The [AEP](https://beyond10x.github.io/docs/aep/) store under `.engineering/planning/` owns
lifecycle state. A story is not complete because the workspace builds.

## What a green gate does not establish

It does not establish live provider access, a working subscription presentation, OS keychain
service availability, hosting-provider qualification or a released artifact. Local fixture evidence
is local fixture evidence. See [Limitations](../status/limitations.md).
