---
format: aep.planning-md/3
id: story:parity-http-timeouts-cancel
kind: story
status: draft
title: llm-http timeouts and cancellation are pinned as Harness pins them
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
revision: 1
---
## Outcome

llm-http timeouts and cancellation are pinned as Harness pins them.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- H21: llm-http offers a connect timeout and an idle limit long enough for a silent long think
- H25: llm-http tests that a cancelled `post_sse` never sends
- H26: llm-http tests a cancel issued while waiting for response headers
- M32: llm-messages tests a connection that keeps sending after `message_stop`
- M46: llm-messages tests a cancel while the server is still sending to an accepting sink

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
