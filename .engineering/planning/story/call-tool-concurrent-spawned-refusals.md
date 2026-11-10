---
format: aep.planning-md/3
id: story:call-tool-concurrent-spawned-refusals
kind: story
status: draft
title: Two concurrent call_tool calls on one spawning model each keep their credential refusal
summary: 'Pre-existing: a second call clears the first call''s spawned refusal'
tags:
- from-finding
relations:
- informed_by: review-result:adversary-github-zero-u4-pass-1
- serves: vision:portable-model-inference
revision: 1
---
## Context

From `review-result:adversary-github-zero-u4-pass-1`, finding 3 (CONFIRMED, pre-existing): two concurrent `call_tool` calls on one model that runs its turn on `tokio::spawn` share the shared refusal slot, so the second call's start clears the first call's spawned refusal and the first comes back as `ModelError::Model` (`crates/llm-tool-call/src/lib.rs:313` at `fd54df39`). What reaches it: nothing found (no caller shares one spawning model across concurrent calls).

## Outcome

A credential refusal keeps its class for each of two concurrent `call_tool` calls on one spawning model.

## Acceptance

`crates/llm-tool-call/tests/adversary_w2026_11_09_u4.rs::two_concurrent_spawned_refusals_are_both_credential_refusals`, which today pins the current behaviour by this story's id, asserts both calls return `MissingCredential` and passes.
