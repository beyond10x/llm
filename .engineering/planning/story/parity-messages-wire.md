---
format: aep.planning-md/3
id: story:parity-messages-wire
kind: story
status: implemented
title: llm-messages caching, refusals and unknown events match Harness
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-messages
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: spec
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T00:56:25Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T00:56:25Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T01:57:09Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Outcome

llm-messages caching, refusals and unknown events match Harness.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- M14, M15: llm-messages places prompt-cache breakpoints on `system` and the conversation tail
- M19: llm-messages tests the tool-name refusal on egress
- M21: llm-messages tests the refusal of a conversation that does not open with the person
- M23, M36: llm-messages preserves an unknown event or content block with a warning instead of ending the turn
- M39: llm-messages tests every pre-flight refusal through the client before anything is sent

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
