---
format: aep.planning-md/3
id: story:parity-responses-live-stream
kind: story
status: implemented
title: llm-responses streams events live and keeps text the caller was shown
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-responses
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: spec
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T02:15:24Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T02:15:25Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T07:35:04Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Outcome

llm-responses streams events live and keeps text the caller was shown.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- R30: llm-responses streams events to the caller while the response arrives
- R36: llm's Responses decoder carries text the caller was shown when the terminal output omits it

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
