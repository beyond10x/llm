---
format: aep.planning-md/3
id: review-result:adversary-w11-llama-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:a-llama-server-response-fixture, pass 1: a refused contradiction keeps a count'
relations:
- reviews: story:a-llama-server-response-fixture
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 1 on `story:a-llama-server-response-fixture`, head
`5010e372` (base `fdce4501`). The report as returned, paths outside the repository shortened.

```
unit: story:a-llama-server-response-fixture at 5010e372 (plus one untracked test file)
verdict: INFEASIBLE (strongest row; no row is NEEDS-CHANGE)
cases: executed 1068→1070, red 1
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: whether to apply the named fix for a constructed state; whether accepted-and-not-read meets "never dropped"
```

## Cases

In `crates/llm-chat/tests/adversary_w11_llama_timings.rs`:

- red `adversary_w11_a_contradiction_refusal_does_not_report_the_contradicted_count`: a refusal of
  timings that contradict usage still carries usage's output count in its evidence.
- green `adversary_w11_a_cached_count_alone_contradicting_usage_is_refused` (kills the mutant
  deleting the cached comparison from `agree`, which survived the unit's suite and all 888
  scenarios).

## Attacked and not broken

Fixture fields, order and types against llama.cpp at 7879491 and inference's recordings (all 12
rates recompute exactly); `prompt_n + cache_n = prompt_tokens` in source and recordings; the shape
check refuses nothing the server writes, `timings_per_token` and progress partials included; a
later usage chunk that contradicts earlier timings is refused; no path turns an absent counter
into zero; servers without `timings` unchanged; the suite regenerates byte-identical; refusals carry
names, never values.

```findings
- file: crates/llm-chat/tests/adversary_w11_llama_timings.rs
  line: 135
  category: contract-drift
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "a refusal of timings that contradict usage still attaches a snapshot reporting usage's count, against the documented rule that neither is believed; read_reported stores both before agree runs"
- file: crates/llm-chat/src/incoming.rs
  line: 533
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "deleting the cached-count comparison from agree passes every llm-chat test and all 888 conformance scenarios; the adversary case now pins it"
- file: crates/llm-chat/src/incoming.rs
  line: 475
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "the eight timings fields accepted and not read reach no caller, while the story acceptance says a field llama-server adds is carried or refused and never dropped"
```

## Routing

- `:135`: back to the implementor; usage and timings are checked to agree before either is stored.
- `incoming.rs:533`: the adversary's case stays in the unit.
- `incoming.rs:475`: the coordinator's decision, recorded on the story: the eight fields are
  accepted and not read with a shape check, the precedent of `UNCONSUMED` in `b10x-llm-messages`;
  refusing them would refuse every llama-server turn, and the neutral `Usage` has no place for
  durations and rates.
