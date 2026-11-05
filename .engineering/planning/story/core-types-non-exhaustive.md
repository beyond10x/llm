---
format: aep.planning-md/3
id: story:core-types-non-exhaustive
kind: story
status: implemented
title: Item, Capabilities, Rejection and AwsCredentialKind can grow without breaking a consumer
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T01:52:20Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T01:52:20Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-05T05:25:14Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

The breaking minor of `approval-record:image-input-breaking-minor` (option B) makes the public types
that keep growing extensible, so a later addition is additive. Today none of them is
`#[non_exhaustive]`, except `RefusalCause` (`crates/llm-core/src/error.rs:40`):

- `Item` (`crates/llm-core/src/item.rs:17`): consumers match it exhaustively;
- `Capabilities` (`crates/llm-core/src/vocabulary.rs:59`): no `Default`, built by struct literal
  downstream;
- `Rejection` (`crates/llm-routing/src/selection.rs:10`);
- `AwsCredentialKind` (`crates/llm-models/src/routes.rs:760`).

## Outcome

Adding an item kind, a capability, a rejection or an AWS credential kind no longer breaks a consumer.

## Acceptance

- `Item`, `Rejection` and `AwsCredentialKind` are `#[non_exhaustive]`. Every match in this workspace
  still names each variant it handles; a wildcard arm only refuses, and never silently drops or sends
  an item.
- `Capabilities` is `#[non_exhaustive]` and gets a constructor and builder methods, or `Default`, so
  a caller outside the crate can still build one. Its serialized documents keep their bytes.
- Released as **Breaking** in the CHANGELOG, with the consumer cost: harness adds a wildcard `Item`
  arm at its own pin move, and control-plane adapts at its own move.
- No other public signature changes.
