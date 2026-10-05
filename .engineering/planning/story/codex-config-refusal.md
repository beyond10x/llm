---
format: aep.planning-md/3
id: story:codex-config-refusal
kind: story
status: implemented
title: A misconfigured Codex login file is refused, not fallen back from
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-credentials/src/codex.rs crates/llm-routing/tests spec contracts docs/local-secrets.md
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T07:37:30Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T07:37:30Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T08:03:22Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Outcome

A Codex login file that cannot be read or whose `exp` cannot be parsed is a configuration error the caller sees as `Unauthorized`, which fallback never takes, as a malformed JSON-pointer document already is.

## Why

Wave 2026-10-05-w22 (`story:parity-credential-sources`): `SecretError::Malformed` now keeps a misconfigured pointer from silently moving to the next route target. In `crates/llm-credentials/src/codex.rs` an unreadable `auth.json` or a bad `exp` still maps to `Unavailable`, which `crates/llm-routing/src/fallback.rs` treats as eligible. The Codex tests and docs pin `Unavailable` today, so changing it is a decision of its own.

## Acceptance

- Each case is decided (Malformed or Unavailable) with its reason in `docs/local-secrets.md`.
- A routing test shows a misconfigured Codex login file does not fall back.
