---
format: aep.planning-md/3
id: story:durable-shadow-cap
kind: story
status: draft
title: The shadow daily cap survives a restart and a second process
tags:
- from-finding
relations:
- informed_by: review-result:security-seams-u4-pass-1
- decomposes: epic:routing
revision: 1
---
## Context

`review-result:security-seams-u4-pass-1` found that the shadow daily cap
(`crates/llm-calls/src/shadow.rs`, `ShadowLedger`) lives in an in-memory budget engine, which
`crates/llm-cost/src/budget/engine.rs:9-10,41` documents as not a durable permission. A restart
during a UTC day, or a second process, starts each day's cap afresh. In the wave of 2026-11-05 the
cap is documented as per process; nothing in this repository runs a long-lived router on it.

## Outcome

A deployment can back each shadow's daily cap with the durable `SqliteLedger` (the
`spending-limits` capability), so a restart or a second process sharing the ledger file cannot
spend past the day's cap.

## Acceptance (draft)

- A shadow ledger opened on a durable ledger keeps a day's spend across a reopen; a scenario
  restarts mid-day and the next shadow past the cap is skipped.
- Two processes on one ledger file cannot both take the last reservation.
