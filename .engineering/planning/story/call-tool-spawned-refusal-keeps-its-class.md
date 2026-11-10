---
format: aep.planning-md/3
id: story:call-tool-spawned-refusal-keeps-its-class
kind: story
status: implemented
title: A credential refusal keeps its class through call_tool on a spawned task
summary: A model turn run on tokio::spawn turns MissingCredential into ModelError::Model
tags:
- from-finding
relations:
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-core/src/error.rs
- confidence: inferred
  path: crates/llm-providers/src/auth.rs
- confidence: cited
  path: crates/llm-tool-call/src/lib.rs
- confidence: cited
  path: crates/llm-tool-call/tests/adversary_model.rs
revision: 12
transitions:
- {from: "draft", to: "proposed", at: "2026-11-10T00:19:24Z", actor: "human:timo", revision: 8}
- {from: "proposed", to: "active", at: "2026-11-10T00:19:24Z", actor: "human:timo", revision: 9}
- {from: "active", to: "implemented", at: "2026-11-10T04:21:40Z", actor: "human:timo", revision: 12, decided_on: {"recorded":{"test_result":1,"review_outcome":3}}}
---
## Context

`crates/llm-tool-call/tests/adversary_model.rs:608`: `a_refusal_on_a_spawned_task_is_still_a_credential_refusal` is `#[ignore = "declined in wave 2026-10-04-w17 …"]`, and `story:review-2026-10-21-ingress-strictness` declined it again. A model that runs its turn on `tokio::spawn` and is refused for a missing login comes back from `call_tool` as `ModelError::Model`, not `MissingCredential`: the refusal is kept in a task-local slot (`crates/llm-tool-call/src/lib.rs:214-225`, the limit documented at `:262`). Re-read at `c10908eb` by `aep:plan-reviewer` on 2026-11-10: still holds. A caller that routes on the credential class (asks for a login) misses it.

## Outcome

A missing-credential refusal keeps its class through `call_tool` whichever task the model ran its turn on.

## Acceptance

`a_refusal_on_a_spawned_task_is_still_a_credential_refusal` runs without `#[ignore]` and passes, and no other `llm-tool-call` test changes its assertion.

## Scope

Derived 2026-11-10 by `aep:story-scoper` at `c10908eb`; confidence high for `llm-tool-call`, medium for the rest.

- cited: `crates/llm-tool-call/src/lib.rs:214-284` (`LOGIN_REFUSAL` task-local slot, `CodexLogin::resolve`, `call_tool`, `classify`), `crates/llm-tool-call/tests/adversary_model.rs:607-621`.
- inferred: `crates/llm-core/src/error.rs:41-79` (`Error` has no credential class; `RefusalCause` is `#[non_exhaustive]`, so a new variant is not a Rust break, while a new `Error` field is), `crates/llm-providers/src/auth.rs:113-131` (`secret_error`, where the class is lost), `CHANGELOG.md`.
- Options (scoper): a `RefusalCause` variant set from the typed secret error (not breaking; stretches its doc), a new `Error` field (breaking), or message classification inside `llm-tool-call` (not breaking, by prose).
- landed (wave 2026-11-09-github-zero, branch head `2a6fbca4`, from `git diff --name-only`): `crates/llm-tool-call/src/lib.rs`, `crates/llm-tool-call/tests/adversary_model.rs`, `crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs`, `CHANGELOG.md` (coordinator).
