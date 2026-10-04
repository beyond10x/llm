---
format: aep.planning-md/3
id: story:parity-blocking-adapters
kind: story
status: draft
title: llm offers blocking Responses and Messages turn adapters for a synchronous loop
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
revision: 1
---
## Outcome

llm offers blocking Responses and Messages turn adapters for a synchronous loop.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- M41: llm offers a blocking Messages turn adapter and tests two concurrent turns on one Messages client
- R40: llm offers a blocking Responses turn adapter for a synchronous agent loop

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
