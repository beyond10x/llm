---
format: aep.planning-md/3
id: review-result:adversary-seams-u1-pass-1
kind: review-result
status: active
title: 'Wave 2026-11-05-llm-seams adversary, story:call-records, pass 1: two introduced'
relations:
- reviews: story:call-records
revision: 1
---
## Adversary pass 1, unit 1 (story:call-records)

Attacked `impl/call-records` at `b0bd124c9`; cases committed as `8e3d9645b`
(`crates/llm-calls/tests/adversary_u1_attempts.rs`, 3 cases, 2 red).

Verdict: red. Cases executed 8 → 11, red 2. Origin: introduced 2, pre-existing 0, undecided 0.

- F1 (blocker, confirmed, introduced), `crates/llm-calls/src/lib.rs:126`: the cost prices only the
  last attempt. Routing retries or falls back after a retriable failure whatever its dispatch, accepted
  included (`crates/llm-routing/src/fallback.rs:529-531`); reached by a Responses `server_error` or
  `server_is_overloaded` before output (`crates/llm-responses/src/stream.rs:777-779`, `Unavailable`,
  `Dispatch::Accepted`, retriable) under the default policy. Case
  `a_retried_accepted_attempt_with_unknown_usage_leaves_the_call_cost_incomplete` is red: `complete: true`.
- F2 (warning, confirmed, introduced), `crates/llm-calls/src/lib.rs:123`: the record's dispatch is the
  run's final error, which after a limit refusal, deadline or cancel following an accepted attempt is the
  default `NotSent`. Reached by the caller's `admit` port (`fallback.rs:344`). Case
  `a_limit_refusal_after_an_accepted_attempt_is_not_recorded_as_not_sent` is red.
- Note (suspected), `crates/llm-calls/src/lib.rs:119`: nothing reached `Outcome::Cancelled` before this
  pass; now guarded by the green case
  `a_run_cancelled_before_its_first_attempt_is_recorded_cancelled_without_a_target`.

Attacked and not broken: no text in the record; unknown usage stays `None`; an unpriced model gives an
incomplete cost, never zero; warnings do not start time to first event; the run is returned unchanged;
attempt count and last target under same-target retry; a run refused before any attempt.

Findings block as the adversary corrected it after the store refused the first verdict words:

```findings
[{"file": "crates/llm-calls/src/lib.rs", "line": 126, "category": "acceptance", "severity": "blocker", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "cost prices only the last attempt, so a call with a retried accepted attempt of unknown usage records complete: true (a_retried_accepted_attempt_with_unknown_usage_leaves_the_call_cost_incomplete)"}, {"file": "crates/llm-calls/src/lib.rs", "line": 123, "category": "judgement", "severity": "warning", "verdict": "CONFIRMED", "origin": "introduced", "message": "record dispatch is the run's final error, so a limit refusal after an accepted attempt records not-sent (a_limit_refusal_after_an_accepted_attempt_is_not_recorded_as_not_sent)"}, {"file": "crates/llm-calls/src/lib.rs", "line": 119, "category": "mutant", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "no existing case or scenario reached Outcome::Cancelled, so mapping it to Failed stayed green; guarded now by a green case"}]
```

