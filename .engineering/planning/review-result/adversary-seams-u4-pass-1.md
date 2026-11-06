---
format: aep.planning-md/3
id: review-result:adversary-seams-u4-pass-1
kind: review-result
status: active
title: 'Wave 2026-11-05-llm-seams adversary, story:shadow-calls, pass 1: three introduced'
relations:
- reviews: story:shadow-calls
revision: 1
---
## Adversary pass 1, unit 3 (story:shadow-calls)

Attacked `impl/shadow-calls` at `aebec66e6`; cases committed as `5d50fd962`
(`crates/llm-calls/tests/adversary_u4_shadow.rs`, 3 cases, 2 red).

Verdict: red. Cases executed 14 → 17, red 2. Origin: introduced 3, pre-existing 0, undecided 0.

- A primary refused by the caller admission hook before any attempt still gets its shadow sent and paid (`shadow.rs:494`).
- A primary record id near the 256-byte limit makes the shadow `Refused(InvalidRequest)` with no record (`shadow.rs:283`); no caller found that sets such ids.
- The midnight test crosses only 23:59 to 00:00, so an hourly day length stays green (`shadow.rs:35`); the green case `the_daily_cap_holds_from_the_first_to_the_last_hour_of_one_utc_day` now pins it.

Not broken: shadow events and errors never reach the caller sink; nothing is reserved or sent before the handle is driven; the primary cannot change because of a shadow; role and primary record id are set on sent and skipped records; only shadows of the primary route follow it; a run across midnight settles without losing spend.

```findings
[{"file": "crates/llm-calls/src/shadow.rs", "line": 494, "category": "acceptance", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "a primary refused by the caller's admit hook before any attempt still gets its shadow sent and paid (adversary_u4_shadow::a_shadow_does_not_follow_a_primary_the_callers_admission_refused_before_sending)"}, {"file": "crates/llm-calls/src/shadow.rs", "line": 283, "category": "boundary", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "a primary record id near the 256-byte limit makes the shadow Refused(InvalidRequest) with no record, misreported as a route refusal; no caller found that sets such ids"}, {"file": "crates/llm-calls/src/shadow.rs", "line": 35, "category": "mutant", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "unit tests and the midnight scenario cross only 23:59 to 00:00, so an hourly DAY_MS keeps them green; pin the day length with a same-day early/late case"}]
```
