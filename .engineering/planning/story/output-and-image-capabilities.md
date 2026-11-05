---
format: aep.planning-md/3
id: story:output-and-image-capabilities
kind: story
status: implemented
title: Catalog capabilities and selection for structured output and image input
tags:
- from-adopter-report
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
- depends_on: story:image-input
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T01:52:02Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T01:52:02Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-05T05:25:15Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

`story:structured-output` and `story:image-input` first asked for a catalog capability per feature
and a named selection rejection, so no route or fallback sends such a turn to a model without the
feature. Wave 2026-11-04-w25 runs additive only (approval-record:wave-2026-11-04-w25-proposal), and
both need a breaking change: `Capabilities` (`crates/llm-core/src/vocabulary.rs:59`) derives no
`Default` and is built by struct literal downstream, and `Rejection`
(`crates/llm-routing/src/selection.rs:10`) is an exhaustive public enum.

## Outcome

Selection knows which models take structured output and image input, and never routes or falls back
to one that does not.

## Acceptance

- A catalog declares per model whether it takes structured output and image input; a document from
  before the fields means not declared, and the bytes of such a document are unchanged.
- Selection rejects a target that declares the feature `false` by a named rejection, and no fallback
  moves such a turn to it; an undeclared feature is not rejected, and the wire carries or refuses the
  turn as before. `Catalog::explain` and the "route has no compatible target" refusal name the
  rejection. (Amended 2026-11-05 by the wave lead: llm has no `llm route explain` command; the command
  is `story:cli-route-explain`.)
- Released together with any other breaking change to `llm-core`, with `#[non_exhaustive]` or a
  `Default` added so the next such field is additive.
