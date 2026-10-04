---
format: aep.planning-md/3
id: story:parity-responses-live-stream
kind: story
status: draft
title: llm-responses streams events live and keeps text the caller was shown
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
revision: 1
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
