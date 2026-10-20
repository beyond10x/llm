---
format: aep.planning-md/3
id: review-result:adversary-w09-retry-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:retry-classes-harness-retried, pass 2: one uncited rationale'
relations:
- reviews: story:retry-classes-harness-retried
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 2 on `story:retry-classes-harness-retried`, head
`2118819a` (correction round 1 on `10168736`). The report as returned, paths outside the repository
shortened.

```
unit: story:retry-classes-harness-retried at 2118819a, plus 2 untracked pass-2 test files
verdict: CONFIRMED (one note-level doc finding; no case came back red)
cases: executed 516→520, red 0
origin: introduced 1 / pre-existing 0 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: none
```

## Cases (all green)

- `crates/llm-responses/tests/adversary_w09_retry_pass2.rs`: all 12 rows of the new ingress
  failures table, each with both retry marks, read back as documented; after streamed output a
  relayed retriable failure reads back final.
- `crates/llm-messages/tests/adversary_w09_retry_pass2.rs`: all 11 codes with both marks read back
  as documented; the same converse after output.

Suite over four crates: 520 executed, 0 red, exit 0; the five AGENTS.md invariant tests and the
pass-1 `response.queued` case green.

## Finding

The rationale added to `docs/design.md` for accepting that a relayed final failure reads back
retriable ("vendor clients already retry these provider codes") cites nothing and is false for the
official SDKs: openai-python and anthropic-sdk-python decide retries from the HTTP status only and
raise an in-stream `error` event without retrying; Anthropic's errors page says an error after a
200 "doesn't follow these standard mechanisms". Codex CLI does class `server_error` and unknown
codes as retryable (`codex-api/src/sse/responses_error.rs:87-103`).

## Attacked and not broken

The ingress tables and prose, the `fail` docs, the protocol pages and all 16 parity citations
match the code; accepting `response.queued` lets a queued attempt be retried and fall back instead
of halting as visible output; both cited stories exist and own what the docs say.

```findings
- file: docs/design.md
  line: 182
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the rationale "vendor clients already retry these provider codes" is uncited and false for openai-python and anthropic-sdk-python, which never retry an error inside a 200 stream; only Codex CLI classes these codes retryable
```
