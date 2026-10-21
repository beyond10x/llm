---
format: aep.planning-md/3
id: approval-record:registry-publication-2026-10-22
kind: approval-record
status: draft
title: No registry publication of llm's crates, decided by the owner on 2026-10-22
summary: 'Option A: llm''s crates stay publish = false and consumers depend by Git release tag'
relations:
- decides: story:crates-from-a-registry
- decides: decision-blocker:registry-publication
revision: 2
---
## The stop

decision-blocker:registry-publication: whether llm's library crates are published to a registry, and to
which one.

## What authorises passing it

The owner, 2026-10-22 01:xx, on morning-queue row 34, relayed by session `b10x-chef-orchestrator`: "33+34
as you recommend". The recommendation on row 34 was option A: do not publish crates to a registry.

## Decision

- No llm crate is published to crates.io or any other registry. The workspace keeps `publish = false`
  (`Cargo.toml`).
- Consumers depend on llm from Git at a release tag, as README.md's "Depend on it" section says.
- The optional `secrets` Git dependency of `b10x-llm-credentials` stays as it is; no registry constraint
  applies to it.
- The release procedure in AGENTS.md stays a source release with no assets.

This clears decision-blocker:registry-publication and archives story:crates-from-a-registry with this
reason. Publishing later needs a new decision and a new story.
