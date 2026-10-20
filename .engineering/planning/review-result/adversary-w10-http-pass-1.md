---
format: aep.planning-md/3
id: review-result:adversary-w10-http-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w10 adversary, unit http, pass 1: a quota named by error.type stays retriable as a 429'
relations:
- reviews: story:quota-429-is-final
- reviews: story:relayed-failures-keep-their-retry-decision
revision: 1
---
## Pass

Wave 2026-10-20-w10, `aep:adversary` pass 1 on unit http (`story:quota-429-is-final` at
`6da06c03`, `story:relayed-failures-keep-their-retry-decision` at `c0ed9f3d`; base `5e00283d`). The
report as returned, paths outside the repository shortened.

```
unit: http at c0ed9f3d
verdict: NEEDS-CHANGE
cases: executed 992→1000, red 3
origin: introduced 4, pre-existing 0, undecided 1
wrote-outside-worktree: the assigned scratch, with a 735M build dir for two mutants
needs-coordinator: the Responses quota rule reads error.code only; the Codex CLI at the commit docs/design.md cites also reads error.type
```

## Cases

- red `crates/llm-responses/tests/adversary_w10_quota_type.rs::a_quota_named_by_its_error_type_is_final_as_a_429_as_it_is_inside_a_stream`:
  `{"type": "insufficient_quota"}` with no code is final inside a stream and a retriable
  `RateLimited` as a 429.
- red `…::a_codex_backend_usage_limit_429_is_final` (`usage_limit_reached`, `usage_not_included`).
- red `crates/llm-models/tests/adversary_w10_usage_limit.rs::a_subscription_usage_limit_429_is_asked_once_and_ends_the_run`:
  routing sent 4 requests, then would fall back.
- green `crates/llm-http/tests/adversary_w10_refusal_bounds.rs`, 5 probes (trickle, exactly 16 KiB
  declared, chunked, gzip, the signal on the wire); two kill mutants the unit's tests miss
  (`length >= MAX_REFUSAL_BYTES`; `REFUSAL_WAIT` 3 s).

Suite with the cases: 997 passed, 3 failed, 3 ignored, exit 101.

## Attacked and not broken

A trickling body is cut at about 1 s; 16 KiB + 1 keeps its class; gzip is never decoded; a non-JSON
body keeps its class; no provider prose reaches a diagnostic; the tap sees the head only.
Cancellation during the body read is checked by routing before a retry. `x-should-retry`: `False`
is no signal, `" false "` is, two fields are no signal, `true` never widens, a 200 stream ignores it.
A non-retriable 502 with dispatch `unknown` halts as `AmbiguousDispatch`. Every parity citation
lands; the spend-cap quotes match the Anthropic page.

```findings
- file: crates/llm-responses/src/client.rs
  line: 43
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: undecided
  message: a 429 naming its quota by error.type (insufficient_quota with no code, Codex backend usage_limit_reached or usage_not_included) stays retriable, so routing asks 4 times, while the same document in a stream is final and the Codex CLI at the cited commit treats all three as terminal
- file: crates/llm-responses/tests/retry_class.rs
  line: 440
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: any_other_429_stays_a_retriable_rate_limit pins {"code":null,"type":"insufficient_quota"} as retriable, the opposite of the reference client's own test (api_bridge_tests.rs:503)
- file: crates/llm-http/src/transport.rs
  line: 432
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the mutant length >= MAX_REFUSAL_BYTES survives every llm-http test; only adversary_w10_refusal_bounds.rs::a_429_body_declared_at_exactly_the_byte_bound_is_read catches it
- file: crates/llm-http/tests/retry_classes.rs
  line: 466
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the stall test bounds the hold by REFUSAL_WAIT itself, so REFUSAL_WAIT = 3 s passes against the documented 1 s; caught only by the literal bound in adversary_w10_refusal_bounds.rs
- file: docs/design.md
  line: 266
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: 'the sentence "read before any status rule" is false for anthropic-sdk-typescript 04a4c28, whose shouldRetry retries a token-cache 401 (client.ts:1753-1763) before reading x-should-retry; transport.rs:376 repeats it'
```

## Routing

All five back to the implementor (correction round 2): the Responses rule also reads `error.type`
`insufficient_quota`, `usage_limit_reached` and `usage_not_included`, citing the Codex CLI; the
unit's case at `:440` flips; the adversary's three files stay in the unit; the SDK sentence is
corrected.
