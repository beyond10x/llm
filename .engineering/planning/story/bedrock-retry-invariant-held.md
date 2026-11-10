---
format: aep.planning-md/3
id: story:bedrock-retry-invariant-held
kind: story
status: active
title: The Bedrock retry-before-answer invariant is held by llm-bedrock tests on both wires
summary: AGENTS.md marks the Bedrock retry invariant UNMAPPED; InvokeModel and silent output are untested
tags:
- from-finding
relations:
- decomposes: epic:github-zero-2026-11-09
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-bedrock/src/invoke.rs
- confidence: cited
  path: crates/llm-bedrock/tests/client.rs
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-11-10T00:19:23Z", actor: "human:timo", revision: 7}
- {from: "proposed", to: "active", at: "2026-11-10T00:19:24Z", actor: "human:timo", revision: 8}
---
## Context

AGENTS.md invariants table: "A Bedrock turn may be retried only before the model answered: a cut or a throttling exception after content is final" is held only by scenarios and marked `UNMAPPED: no crates/llm-bedrock test holds it yet`. Re-read at `c10908eb` by `aep:plan-reviewer` on 2026-11-10: the code reads right (`crates/llm-bedrock/src/converse.rs:916`, `crates/llm-bedrock/src/client.rs:529-533`); `crates/llm-bedrock/tests/client.rs:468` and `:504` cover Converse with visible text only. Nothing tests `AnthropicInvoke`, or a cut after silent output such as a `contentBlockStart` for a `toolUse`. An invariant no test holds is not a claim (AGENTS.md: "Each claim fails the named test when broken").

## Outcome

The invariant is held by named `llm-bedrock` tests on both Bedrock wires, including after silent output, and the AGENTS.md row names them.

## Acceptance

On both Converse and InvokeModel, a cut and a `throttlingException` after a silent `contentBlockStart` are not retriable, and before any content they are; the tests live in `crates/llm-bedrock/tests/` and the AGENTS.md row names them with no `UNMAPPED` marker.

## Scope

Written 2026-11-10 by the coordinator from the `aep:plan-reviewer` triage at `c10908eb`; confidence medium. No scoper ran.

- cited: `crates/llm-bedrock/tests/client.rs:468`, `:504` (the Converse cases; helper `failed` at `:261-263` is Converse-only), `AGENTS.md` (the UNMAPPED row).
- cited, read only: `crates/llm-bedrock/src/converse.rs:916`, `crates/llm-bedrock/src/client.rs:529-533`.
- inferred: `crates/llm-bedrock/src/invoke.rs` only if InvokeModel turns out not to mark the turn answered on silent output; `CHANGELOG.md`.
- landed (wave 2026-11-09-github-zero, branch head `cb3e115d`, from `git diff --name-only`): `crates/llm-bedrock/tests/adversary_w2026_11_09_u2.rs`, `crates/llm-bedrock/tests/client.rs`, `AGENTS.md` (coordinator).
