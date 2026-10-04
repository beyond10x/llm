---
format: aep.planning-md/3
id: review-result:adversary-w18-llm-codex-stream-pass-1
kind: review-result
status: active
title: Wave 2026-10-04-w18 adversary, llm story:codex-stream, pass 1
relations:
- reviews: story:codex-stream
revision: 1
---
## Adversary pass (single) — llm story:codex-stream

Tree: 1c05fa0f + phase 2. Verdict CONFIRMED; cases 118→134, red 2. New files: `crates/llm-responses/tests/adversary_codex_stream.rs`, `crates/llm-http/tests/adversary_untyped.rs`.

```findings
- file: crates/llm-responses/src/stream.rs
  line: 252
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: an auto-choice empty untyped turn is an empty EndTurn, contradicting the story acceptance wording while matching docs/responses.md
- file: crates/llm-responses/src/stream.rs
  line: 252
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: preferring streamed items over a non-empty terminal output survived all pre-existing cases
- file: crates/llm-responses/src/stream.rs
  line: 286
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: text deltas handed to the caller can end in an EndTurn outcome with no items
- file: crates/llm-http/src/transport.rs
  line: 177
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: an untyped non-SSE 2xx now fails as a stream without a terminal with dispatch Unknown
```

| # | finding | decision |
|---|---|---|
| 1 | stream.rs:252 an auto-choice empty untyped turn is an empty EndTurn; story says refused, docs say empty turn | docs win: an empty auto turn with nothing streamed stays an empty turn (the ~15 existing fixtures rely on it); the story acceptance wording is corrected at close; the adversary case is marked #[ignore] with this reason |
| 2 | stream.rs:252 preferring streamed items over a non-empty terminal output survived all existing cases | accept; the adversary cases now pin the rule; no code change |
| 3 | stream.rs:286 text deltas shown to the caller can end in an EndTurn with no items (pre-existing) | decline in this story: pre-existing and unreachable from any observed server; recorded in the story body as a follow-up; the adversary case is marked #[ignore] with this reason |
| 4 | transport.rs:177 an untyped non-SSE 2xx now fails as a stream without a terminal (dispatch Unknown) | accept as documented behaviour; no change |

Integration gate on 0fb20aad: 8 steps exit 0; 631 tests passed, 3 ignored.
