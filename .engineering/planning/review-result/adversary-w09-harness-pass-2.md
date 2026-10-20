---
format: aep.planning-md/3
id: review-result:adversary-w09-harness-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:harness-cutover-on-llm-0-7, pass 2: pass-1 findings fixed, three notes'
relations:
- reviews: story:harness-cutover-on-llm-0-7
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 2 on `story:harness-cutover-on-llm-0-7`, commit
`3934311d` (correction round 1 on `62f107fd`). The report as returned, paths outside the repository
shortened.

```
unit: story:harness-cutover-on-llm-0-7, commit 3934311d (crates/ at HEAD byte-identical to tag 0.7.0)
verdict: NEEDS-CHANGE (three notes, nothing blocking)
cases: executed 45→45 (cargo gate, from the implementor's logs; checks are scratch scripts outside it), red 2
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: the assigned scratch (three shared clones)
needs-coordinator: none
```

## Pass-1 findings, as corrected

All six fixed: R28 and M27 say `partial` and why; AGENTS.md, the site index and the handed-back
status text name llm-gateway; the header names every row whose text changed; H21 points at H20;
the recorded re-anchoring run now fails on a line outside a row's set (the H6 mutation exits 1).

## New probes

The recorded run keys its reference by (row, path, line, start|end), not by citation, so a
citation moved to another line cited in the same row and file, or to one of that group's hand
entries, still passes: R36 `stream.rs:279` changed to `:388` and M15 `codec.rs:69` changed to
`:294` both exit 0. 139 of 334 line citations sit in 55 such groups.

## Attacked and not broken

264 test, 45 symbol and 334 line citations as stated; 44 hand entries, none unused; 148 row IDs as
at the base; status counts 137/4/0/7; the new bridge paragraph correct at both tags.

```findings
- file: docs/harness-parity.md
  line: 32
  category: mutant
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the recorded run checks a line citation only for membership in its row-file-kind group, so a citation swapped to another line of that group or to one of its hand entries passes with exit 0 (139 of 334 line citations exposed)
- file: docs/harness-parity.md
  line: 154
  category: judgement
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: R28 says the classes reach Harness as it had them, but server_is_overloaded, Refused in Harness, reaches it as a retriable Transport
- file: README.md
  line: 17
  category: contract-drift
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: README lists Harness, Loom and inference as llm's consumers and says they run the loop, while AGENTS.md and the site index name llm-gateway as a consumer, and it runs no loop
```
