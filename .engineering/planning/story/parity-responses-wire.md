---
format: aep.planning-md/3
id: story:parity-responses-wire
kind: story
status: active
title: llm-responses request and stream encoding match Harness where a route needs it
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
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T07:09:59Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T07:09:59Z", actor: "human:timo", revision: 3}
---
## Outcome

llm-responses request and stream encoding match Harness where a route needs it.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- H31: llm-responses exposes the exact request bytes it sends
- R13: llm-responses sends a per-conversation prompt-cache key
- R14: llm-responses asserts that an unset output bound is missing from the body, not sent as `null`
- R18: llm-responses sends a successful string tool result as plain text and a failure under `error`, or records why its `{ok, output}` envelope differs from Harness's
- R25: llm-responses tests that an unknown output item is preserved with a warning
- R3: llm-responses tests a stream that ends with `data: [DONE]`
- R44: llm-responses pins a recorded request and its headers as a fixture
- R8: llm-responses sends conversation-identity headers where a route needs them

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
