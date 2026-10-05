---
format: aep.planning-md/3
id: story:codex-config-refusal
kind: story
status: draft
title: A misconfigured Codex login file is refused, not fallen back from
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

A Codex login file that cannot be read or whose `exp` cannot be parsed is a configuration error the caller sees as `Unauthorized`, which fallback never takes, as a malformed JSON-pointer document already is.

## Why

Wave 2026-10-05-w22 (`story:parity-credential-sources`): `SecretError::Malformed` now keeps a misconfigured pointer from silently moving to the next route target. In `crates/llm-credentials/src/codex.rs` an unreadable `auth.json` or a bad `exp` still maps to `Unavailable`, which `crates/llm-routing/src/fallback.rs` treats as eligible. The Codex tests and docs pin `Unavailable` today, so changing it is a decision of its own.

## Acceptance

- Each case is decided (Malformed or Unavailable) with its reason in `docs/local-secrets.md`.
- A routing test shows a misconfigured Codex login file does not fall back.
