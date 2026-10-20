---
format: aep.planning-md/3
id: story:crates-from-a-registry
kind: story
status: draft
title: Consumers can depend on llm's library crates from a registry, if the owner decides to publish them
summary: Every crate is publish = false; consumers depend by Git tag; publishing is irreversible and the secrets Git dependency blocks it
relations:
- decomposes: epic:contracts
- serves: vision:portable-model-inference
revision: 1
---
## Context

Reported by the `inference` repository (Atlas ADR 0100), which depends on llm by Git tag because no
llm crate is published.

At tag 0.7.0 the workspace sets `publish = false` (`Cargo.toml:27`) and every crate inherits it;
README.md tells consumers to depend from Git at a release tag, and AGENTS.md's release procedure is a
source release with no assets. That is the current design, not a defect.

Publishing to a registry has costs this story has to settle first:

- A version published to crates.io cannot be deleted, only yanked, and the `b10x-llm-*` names
  become public.
- `b10x-llm-credentials` has optional Git dependencies on the `secrets` library at tag `v0.5.0`
  (`crates/llm-credentials/Cargo.toml:40-41`); a registry refuses a crate whose dependencies are not
  themselves published, so `secrets` would have to publish too, or the feature leaves the published
  crate.
- The release procedure in AGENTS.md gains a publish step and its verification.

## Outcome

The owner decides whether llm's library crates are published, and to which registry. If yes, a
consumer depends on them by version from that registry, and each release publishes them.

## Acceptance

- The owner's decision is recorded as an approval record in this store.
- If publishing: every library crate except `llm-docs`, `llm-cli` and the conformance runner
  publishes from a release tag; `cargo publish --dry-run` for each is part of the gate; README.md and
  AGENTS.md's release section say how; the `secrets` dependency is resolved as decided.
- If not: README.md states that Git tags are the supported way to depend on llm, and this story is
  archived with the reason.

## Scope (inferred)

`Cargo.toml`, every crate manifest, `crates/llm-credentials/Cargo.toml`, `Taskfile.yml`,
`.github/workflows/`, README.md, AGENTS.md.
