---
format: aep.planning-md/3
id: review-result:adversary-w10-http-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w10 adversary, unit http, pass 2: nothing found'
relations:
- reviews: story:quota-429-is-final
- reviews: story:relayed-failures-keep-their-retry-decision
revision: 1
---
## Pass

Wave 2026-10-20-w10, `aep:adversary` pass 2 on unit http at `1ba72135` (pass 1 at `c0ed9f3d`;
answered by `fd17bf92` and `1ba72135`). The report as returned, paths outside the repository
shortened.

```
unit: http at 1ba72135
verdict: nothing found
cases: executed 1001→1003, red 0
origin: introduced 0, pre-existing 0, undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: no
```

## Cases

`crates/llm-responses/tests/adversary_w10_quota_type_pass2.rs`, both green on the first run and
kept in the unit (`3d57d094`):

- `a_rate_limit_near_a_quota_type_stays_retriable`: 15 bodies that only resemble a quota stay a
  retriable rate limit (a type containing a quota word, a padded type, quota words only in the
  message, a non-string type, a quota type at the top level or under `error.details`, `error` as a
  string or array, a top-level array).
- `every_codex_429_fixture_reads_as_codex_reads_it`: 19 bodies, 15 from the Codex CLI's own tests at
  `8196bcdc` and 4 combinations read from `api_bridge.rs:192` and `:225-226`.

Suite: 1003 passed, 0 failed, 3 ignored, exit 0.

## Attacked and not broken

No rate limit reads final. Every new citation lands (Codex `api_bridge.rs:185-238`,
`error.rs:410-437`, `api_bridge_tests.rs:503`; openai-python `_base_client.py:900-907`;
anthropic-sdk-typescript `client.ts:1753-1763`; the eight line numbers in `docs/harness-parity.md`).
The "type wins" rule matches the OR at `api_bridge.rs:225-226`. Remaining differences from Codex,
read in its source and outside this story: on a malformed body (duplicate key, non-string sibling
`code`, non-integer `resets_at`) Codex's typed parse fails and it retries where llm reads final;
`flex_unavailable` is terminal in Codex and retriable here (flex capacity, not a quota).

```findings
[]
```
