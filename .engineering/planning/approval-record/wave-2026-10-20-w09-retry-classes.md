---
format: aep.planning-md/3
id: approval-record:wave-2026-10-20-w09-retry-classes
kind: approval-record
status: draft
title: story:retry-classes-harness-retried joins wave 2026-10-20-w09, approved under the owner's grant to adr-changes
tags:
- delegate-approved
relations:
- decides: story:retry-classes-harness-retried
revision: 1
---
## The stop

Adding a unit to wave 2026-10-20-w09 after its approval.

## What authorises passing it

Session `adr-changes` (formerly `takt-monitor`) approved on 2026-10-20 that
`story:retry-classes-harness-retried` joins wave 2026-10-20-w09 so the next release carries it. The
owner's standing grant of 2026-10-20 makes that session's approvals for work implementing Atlas
ADRs 0096 to 0101 count as the owner's (this story serves the Harness cutover of ADR 0097). The
approval covers the same kinds of commits as `approval-record:wave-2026-10-20-w09-proposal`: the
unit's commits on `impl/retry-classes-harness-retried`, its merge into `wave/2026-10-20-w09`, and
the wave's existing integration, closing and merge commits.

## The decision it carries

Mark all four classes retriable before any output and final after output, the shape of the
existing overload rule: Responses `server_error` and `rate_limit_exceeded`; Messages
`overloaded_error` and `api_error`. `docs/design.md` cites the provider documentation that lists
each as transient. A class a provider page says not to retry stays final, and R28 or M27 says why.

## Placement

Layer 4, after `story:tool-result-rejected-or-failed`: it edits `crates/llm-responses/src/stream.rs`
and `crates/llm-messages/src/decode.rs`, which the layer-2 unit also edits.
