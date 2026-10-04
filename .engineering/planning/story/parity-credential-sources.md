---
format: aep.planning-md/3
id: story:parity-credential-sources
kind: story
status: draft
title: llm-credentials reads every token source Harness reads
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
revision: 1
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
