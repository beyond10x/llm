---
format: aep.planning-md/3
id: story:parity-retry-classes
kind: story
status: draft
title: llm retries the failure classes Harness retries, before any output is visible
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
revision: 1
---
## Outcome

llm retries the failure classes Harness retries, before any output is visible.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- H13: llm-http pins every HTTP status Harness maps (403, 408, 409, 400, 529) with a named test, and offers 408, 5xx and 529 for another attempt as Harness does (today only 429 falls back: the others are `Transport`/`Unknown`, which fallback refuses)
- H15, H16, H17, H19, R42: llm retries one target before any output is visible, with a capped, cancellable back-off and a stated retry warning
- H18: llm applies the visible-output rule to a same-target retry as well as to fallback
- H20: llm caps a server-requested retry delay and waits on it inside the retry
- H23: llm lets a caller retry the classes Harness retries (today only 429 falls back; 408, 5xx and 529 are `Transport`/`Unknown` and a truncated stream is `Accepted`, all refused by fallback), and states them without a routing table
- H8: llm offers a turn whose stream was cut before any output was shown for another attempt (today `SseStream` stamps it `Accepted`, never eligible)

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
