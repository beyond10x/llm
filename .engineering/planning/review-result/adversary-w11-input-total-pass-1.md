---
format: aep.planning-md/3
id: review-result:adversary-w11-input-total-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w11 adversary, story:messages-input-total-derived-from-what-llm-observed, pass 1: a later lower count lowers the total'
relations:
- reviews: story:messages-input-total-derived-from-what-llm-observed
revision: 1
---
## Pass

Wave 2026-10-20-w11, `aep:adversary` pass 1 on `story:messages-input-total-derived-from-what-llm-observed`,
head `b93c6ff9` (base `640fff2b`). The report as returned, paths outside the repository shortened.

```
unit: story:messages-input-total-derived-from-what-llm-observed at b93c6ff9, plus one untracked adversary test file
verdict: NEEDS-CHANGE (the gate is red on one added case; INFEASIBLE, no upstream shown to reach it)
cases: executed 1049→1052, red 1
origin: introduced 4, pre-existing 0, undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: the red case is a state no known upstream sends; the story's Outcome line is not met by the unit's own account
```

## Cases

In `crates/llm-messages/tests/adversary_w11_u3_input_total.rs`:

- red `a_reported_write_count_below_the_derived_one_does_not_lower_the_known_total`: a breakdown of
  0+6 on `message_start`, then a reported 3 on `message_delta`, lowers the write count 6→3 and the
  total 21→18 (the same pair in one event is refused).
- green `a_breakdown_that_is_the_only_counter_is_a_known_write_count` (kills the mutant deleting
  the breakdown from `is_known`).
- green `a_total_with_a_derived_write_count_reaches_u64_max_and_one_more_is_refused`.

## Attacked and not broken

No absent counter reads as zero (missing, null or empty breakdown, a null count, an hour count of 0
with no five-minute count); a breakdown on `message_delta`; a higher reported count after a
breakdown; overflow at `u64::MAX`; the derived count reaches pricing at the five-minute write rate,
which is right since one-hour writes are refused; the quoted reference and Harness lines match.

```findings
- file: crates/llm-messages/tests/adversary_w11_u3_input_total.rs
  line: 109
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: "a cache_creation_input_tokens reported after a derived write count is accepted when lower, so the cumulative write count and the inclusive total go backwards (6 to 3, 21 to 18) where the same pair in one event is refused"
- file: crates/llm-messages/src/usage.rs
  line: 104
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "deleting the added is_known disjunct for the breakdown leaves every unit and existing llm-messages test green; only the added case at adversary_w11_u3_input_total.rs:123 catches it"
- file: docs/harness-parity.md
  line: 204
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: "parity table row M25 still says the total is known only when all three counts are reported and cites usage.rs:95-112"
- file: .engineering/planning/story/messages-input-total-derived-from-what-llm-observed.md
  line: 62
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: "the story Outcome requires a derivation that fires for the runs that reported the gap, and the unit's own parity note says it does not reach the Harness emulator or any known upstream"
```

## Routing

- `:109`: back to the implementor; a later reported count lower than the derived one is refused as
  a cumulative regression, and `docs/messages.md` says so.
- `usage.rs:104`: the adversary's file stays in the unit.
- `docs/harness-parity.md:204`: no change. The matrix rows describe and cite tag 0.7.0 by design; the
  next-release note at M25 states the new rule.
- The story's Outcome: recorded on the story at close. The acceptance bullets are met (the omitting
  upstream named, a documented derivation, tests both ways, no assumed zero); the second clause of
  the Outcome is not, because the only known omitter sends no breakdown, and Harness closed that
  gap on its own side (`36a587c5`).
