---
format: aep.planning-md/3
id: approval-record:wave-2026-10-20-w09-messages-input-total
kind: approval-record
status: draft
title: story:messages-input-total-with-an-absent-cache-counter joins wave 2026-10-20-w09, approved under the owner's grant to adr-changes
tags:
- delegate-approved
relations:
- decides: story:messages-input-total-with-an-absent-cache-counter
revision: 1
---
## The stop

Adding a unit to wave 2026-10-20-w09 after its approval.

## What authorises passing it

Session `adr-changes` approved on 2026-10-20 that `story:messages-input-total-with-an-absent-cache-counter`
joins wave 2026-10-20-w09, with or after `story:retry-classes-harness-retried`, so the next release
carries it. The owner's standing grant of 2026-10-20 makes that session's approvals for work
implementing Atlas ADRs 0096 to 0101 count as the owner's (Loom's metering on llm's `Model`, Atlas
ADR 0098 point 4; the Harness cutover, ADR 0097). Reason given: with the field absent no Messages
run in Harness's cutover files usage or cost (parity row M25), and Harness merges onto 0.8.0 only.
The approval covers the unit's commits, its merge into `wave/2026-10-20-w09`, and the wave's
existing integration, closing and merge commits.

## The decision it carries

An absent `cache_creation_input_tokens` in a Messages usage report counts as 0, as the vendor does;
`docs/messages.md` cites the vendor documentation or code that says so.

This narrows the repository rule "Unknown usage stays unknown, never zero" (AGENTS.md "Never") for
that one Messages counter. The coordinator records the exception in AGENTS.md's invariant row and
in `docs/design.md`; every other absent counter stays unknown.

## Placement

Layer 4, in the same tree as `story:retry-classes-harness-retried` and after it, one commit each:
both touch the Messages decoder and its page.
