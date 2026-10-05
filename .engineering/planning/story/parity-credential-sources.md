---
format: aep.planning-md/3
id: story:parity-credential-sources
kind: story
status: implemented
title: llm-credentials reads every token source Harness reads
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: crates/llm-credentials
- confidence: inferred
  path: crates/llm-providers/src/auth.rs
- confidence: inferred
  path: docs/harness-parity.md
- confidence: inferred
  path: spec
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-04T23:15:32Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-04T23:15:32Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T00:14:29Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Outcome

llm-credentials reads every token source Harness reads.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- C1: llm-credentials reads a token file whose last byte is a newline
- C2: llm-credentials resolves a secret from a caller-named environment variable
- C4: llm-credentials reads a token at a caller-named JSON pointer
- C7: llm-credentials' file source says which reference refused, without its path or value
- C8: llm-providers presents a subscription OAuth token differently from an API bearer

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
