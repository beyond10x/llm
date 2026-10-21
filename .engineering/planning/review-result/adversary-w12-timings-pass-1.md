---
format: aep.planning-md/3
id: review-result:adversary-w12-timings-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-21-w12 adversary, story:timings-before-the-last-chunk, pass 1: notes only'
relations:
- reviews: story:timings-before-the-last-chunk
revision: 1
---
## Pass

Wave 2026-10-21-w12, `aep:adversary` pass 1 on `story:timings-before-the-last-chunk`, head
`4ab4168a` (base `d7f06314`), the adversary's cases committed as `58732dbc`. The report as
returned, paths outside the repository shortened.

```
unit: story:timings-before-the-last-chunk, change d7f06314..4ab4168a, findings cover 58732dbc
verdict: CONFIRMED (notes only, no red case against the unit)
cases: executed 149→153, red 0
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: none
```

## Cases

In `crates/llm-chat/tests/adversary_w12_timings.rs`, all green on the unit:

- `adversary_w12_a_cut_after_per_token_timings_reports_no_count_in_its_evidence`: a stream cut
  after per-token timings fails with evidence whose usage is unknown (red on the old decoder).
- `adversary_w12_a_cut_after_the_finish_chunk_keeps_its_timings_as_the_snapshot`: cut after the
  finish chunk carrying timings, usage {35, 4, 0}, not final.
- `adversary_w12_per_token_timings_without_a_usage_chunk_count_from_the_finish_chunk`: the count is
  the finish chunk's.
- `adversary_w12_a_chunk_with_no_choices_before_the_finish_reason_does_not_end_the_stream`: kills
  the mutant `.any` to `.all` in `finishes`, which survived the unit's suite.

Seven mutants of the decoder: six red under the unit's suite, the seventh red under the last case.
The two new ESS scenarios fail against the old decoder (888 passed, 2 failed) and pass against the
unit (890/890).

## Attacked and not broken

A null, missing or empty-string `finish_reason`; a refused finish reason or one on a choice other
than index 0 (unchanged from the old decoder); a usage chunk before the finish chunk; finish-chunk
timings against a later usage chunk; the non-streamed path; a malformed earlier `timings` is still
refused by field name. llama.cpp `7879491cf` writes timings only on `deltas.back()`
(`server-task.cpp:508-510`), and on partial chunks in per-token mode (`server-context.cpp:2222-2225`).

```findings
- file: crates/llm-chat/src/incoming.rs
  line: 493
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "changing .any to .all in finishes leaves the unit's suite green; the adversary case for a chunk with no choices catches it"
- file: docs/chat.md
  line: 146
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the counters section still says timings is written only on the last chunk and does not say an earlier per-token timings is shape-checked and dropped"
- file: crates/llm-chat/src/incoming.rs
  line: 454
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: "usage and timings on one chunk that does not end the stream are no longer held to agree; llama.cpp writes no usage on partial chunks (server-task.cpp:1103-1121)"
```

## Routing

- `incoming.rs:493`: the adversary's case stays in the unit.
- `docs/chat.md:146`: fixed by the coordinator in the integration commit (the implementor's
  prepared patch, plus the sentence at 142-144).
- `incoming.rs:454`: accepted; the server never writes that chunk, and the design drops a
  `timings` that does not end the stream before any comparison.
