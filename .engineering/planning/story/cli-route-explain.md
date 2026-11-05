---
format: aep.planning-md/3
id: story:cli-route-explain
kind: story
status: draft
title: llm route explain shows which target selection chooses and why
tags:
- from-review
relations:
- decomposes: epic:inference
- depends_on: story:output-and-image-capabilities
revision: 2
---
## Context

`story:output-and-image-capabilities` (wave 2026-11-05-w26) asked that `llm route explain` name a
selection rejection. `llm-cli` has only `model list` and `infer`; the rejection is named in
`Catalog::explain`'s JSON and in the "route has no compatible target" refusal, which a library caller
reads. A command-line user cannot see why a route chose or rejected a target without running a turn.

## Outcome

A command-line user sees, for a route and a turn shape, which target selection would choose and why
each other target was rejected, without calling a provider.

## Acceptance

- `llm route explain <route>` over `Routes::catalog().explain` prints every candidate target with its
  rejection names (for example `structured-output`, `image-input`), in selection order, and the chosen
  target; it makes no network call and prints no secret.
- Flags state the turn's needs (structured output, image input, tools); a test pins the output.
