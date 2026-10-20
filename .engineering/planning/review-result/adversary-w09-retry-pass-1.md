---
format: aep.planning-md/3
id: review-result:adversary-w09-retry-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:retry-classes-harness-retried, pass 1: relayed finality lost, two pre-existing'
relations:
- reviews: story:retry-classes-harness-retried
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 1 on `story:retry-classes-harness-retried`, head
`10168736` (base `51a8f5b4`). The report as returned, paths outside the repository shortened.

```
unit: story:retry-classes-harness-retried at 10168736, plus 3 untracked adversary test files
verdict: NEEDS-CHANGE
cases: executed 508→516, red 4
origin: introduced 2 / pre-existing 2 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: should a failure a gateway marked final come back retriable one hop later; where the two pre-existing reds go
```

## Cases

- red `responses/tests/adversary_w09_retry.rs::a_failure_a_gateway_marked_final_reads_back_final`:
  a final `Unavailable` written by `IngressStream::fail` as `server_error` reads back retriable
  (final at the base); `rate_limit_exceeded` likewise.
- red `messages/tests/adversary_w09_retry.rs::a_failure_a_gateway_marked_final_reads_back_final`:
  a final `Unavailable`, `Transport`, `Protocol` or `Cancelled` reads back retriable (no Messages
  stream error read back retriable at the base).
- red `responses/tests/adversary_w09_retry_http_quota.rs::an_exhausted_quota_answered_as_a_429_before_the_stream_is_final`:
  an HTTP 429 with any of 5 billing codes is `RateLimited`, retriable (pre-existing transport rule).
- red `a_transient_failure_after_every_lifecycle_payload_is_retriable`: `response.queued` is in the
  lifecycle set but not in `ACCEPTED_STREAM_EVENTS`, so it reaches the caller as an
  `unknown-stream-event` warning that routing counts as output (pre-existing).
- green: 3 Responses codes × 8 outputs and 2 Messages types × 9 outputs final after any output, no
  provider text; Messages transient errors after keep-alives retriable; a gateway-retriable mark
  reads back retriable.

Suite over the four crates: 516 executed, 4 red, exit 101; every AGENTS.md invariant test passed.

## Attacked and not broken

All 7 OpenAI and 3 Anthropic quotes in `docs/design.md` appear verbatim on the fetched pages;
classes outside the four stay final; contracts regenerate identical; all 12 parity citations land
and the rows stay at 0.7.0.

```findings
- file: crates/llm-responses/src/stream.rs
  line: 774
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a failure IngressStream::fail writes as final (server_error, rate_limit_exceeded) now reads back retriable when nothing was streamed first, so the retry mark llm-gateway passes in no longer survives one hop
- file: crates/llm-messages/src/decode.rs
  line: 777
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a final unavailable, transport, protocol or cancelled failure written by the Messages IngressStream::fail now reads back retriable, where before this unit no Messages stream error did
- file: crates/llm-http/src/transport.rs
  line: 339
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: pre-existing
  message: an exhausted quota or spend limit answered as HTTP 429, the path OpenAI documents for it, is marked retriable although the story decides it stays final; the fix needs a transport change outside this unit's files
- file: crates/llm-responses/src/stream.rs
  line: 35
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: response.queued counts as not-output for the retry rule but is missing from ACCEPTED_STREAM_EVENTS, so it reaches the caller as an unknown-stream-event warning that routing treats as visible output; llm never requests the background mode that would send it
```
