---
format: aep.planning-md/3
id: story:opencode-go-session-on-every-spelling
kind: story
status: active
title: Every spelling of the OpenCode Go base URL gets the session header or is refused
summary: A trailing-dot host or a percent-encoded path reaches OpenCode Go without x-opencode-session
tags:
- from-finding
relations:
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
- informed_by: review-result:adversary-w19-opencode-go-session-pass-1
scope:
- confidence: cited
  path: crates/llm-messages/src/client.rs
- confidence: inferred
  path: crates/llm-messages/tests/opencode_session.rs
- confidence: inferred
  path: crates/llm-providers/src/declaration.rs
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-11-10T00:19:24Z", actor: "human:timo", revision: 7}
- {from: "proposed", to: "active", at: "2026-11-10T00:19:24Z", actor: "human:timo", revision: 8}
---
## Context

From `review-result:adversary-w19-opencode-go-session-pass-1`, finding F1 (INFEASIBLE / introduced at the time): `crates/llm-messages/src/client.rs:56-57` recognises OpenCode Go by an exact `starts_with("https://opencode.ai/inference/go/")`, and `BaseUrl::new` (`crates/llm-providers/src/declaration.rs:23-40`) neither strips a trailing-dot host nor decodes a percent-encoded path. A base URL spelled `https://opencode.ai./inference/go/anthropic/v1` or `https://opencode.ai/inference/%67o/anthropic/v1` reaches the same route without `x-opencode-session`, which the route refuses with HTTP 400 `MissingSessionID`. Re-read at `c10908eb` by `aep:plan-reviewer` on 2026-11-10: still holds. What reaches it: nothing found (takt and the docs page use the plain spelling); a wrong recognition is still a defect.

## Outcome

Every spelling of the OpenCode Go base URL that addresses the same route gets the session header, or is refused before any I/O by name.

## Acceptance

A Messages client built on `https://opencode.ai./inference/go/anthropic/v1` or `https://opencode.ai/inference/%67o/anthropic/v1` either sends `x-opencode-session` on its request or is refused before I/O, and a test in `crates/llm-messages/tests/` asserts it for both spellings and for an upper-case host.

## Scope

Written 2026-11-10 by the coordinator from the `aep:plan-reviewer` triage at `c10908eb`; confidence medium. No scoper ran.

- cited: `crates/llm-messages/src/client.rs:56-57` (the exact prefix check).
- inferred: `crates/llm-providers/src/declaration.rs:23-40` (`BaseUrl::new`, if normalising there), `crates/llm-messages/tests/opencode_session.rs`, `CHANGELOG.md`.
