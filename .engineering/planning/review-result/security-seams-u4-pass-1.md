---
format: aep.planning-md/3
id: review-result:security-seams-u4-pass-1
kind: review-result
status: active
title: 'Wave 2026-11-05-llm-seams security review, story:shadow-calls, pass 1: cap held, per-process only'
relations:
- reviews: story:shadow-calls
revision: 1
---
## Security review pass 1, unit 3 (story:shadow-calls): the daily money cap

Reviewed `bbdee8d15...aebec66e6`; cases committed as `88b550a05`
(`crates/llm-calls/tests/security_u4_cap.rs`, 3 cases, all green: a 400-sequence fixed-seed property that no UTC day spends past its cap beyond the excess of shadows in flight, self-shadow and shadow-of-shadow routes capped and never chained, a day ledger kept while its shadow is in flight days later).

Verdict: NEEDS-CHANGE (judgement). Cases executed 26 → 29, red 0. Origin: introduced 1.

- The daily cap lives in an in-memory budget engine (`shadow.rs:44-57`), which the engine documents as not a durable permission (`crates/llm-cost/src/budget/engine.rs:9-10,41`); a restart or a second process starts each day afresh. Nothing in this repository builds a long-running router on it.

Could not fault: reserve and start under one lock; settlement over every attempt; unknown or wrong-currency cost held, never zero; checked u64/u128 arithmetic and strict amount parsing; clock set back refused; dropped handle sends or holds as documented; the documented bound is the exact bound.

```findings
[{"file": "crates/llm-calls/src/shadow.rs", "line": 44, "category": "judgement", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "the daily cap lives in an in-memory simulation BudgetEngine, which its own contract (engine.rs:41) says is not a durable permission, so a restart or a second process starts each day's cap afresh; back it with SqliteLedger or document that the cap is per process"}]
```
