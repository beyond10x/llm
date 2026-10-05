---
format: aep.planning-md/3
id: story:call-tool-helper
kind: story
status: draft
title: llm offers call_tool and the Codex preset, replacing Loom's intake-model
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

llm's `call_tool` helper (a forced tool call returning the tool's JSON input) and the Codex Responses preset live in an llm client crate, released by tag, so Loom's intake crates use it instead of carrying `intake-model`.

## Why

Atlas ADR 0090 point 5 and loom `story:import-intake` (changed at wave w25 open): `intake-model` (530 src lines at intake 8d25b09) is a generic llm convenience layer. Loom keeps it until this story releases.

## Acceptance

- An llm crate exposes `call_tool` and the Codex preset with tests ported from `intake-model`.
- An llm release carries it; Loom replaces `crates/intake-model` with that tag and deletes the crate.
