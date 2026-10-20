---
format: aep.planning-md/3
id: review-result:adversary-w10-codex-test-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w10 adversary, story:codex-renewal-test-load-independent, pass 2: the stall bound covers async stalls only'
relations:
- reviews: story:codex-renewal-test-load-independent
revision: 1
---
## Pass

Wave 2026-10-20-w10, `aep:adversary` pass 2 on `story:codex-renewal-test-load-independent`, head
`d240a98f` (pass 1 at `11d527eb`). The report as returned, paths outside the repository shortened.

```
unit: story:codex-renewal-test-load-independent at d240a98f
verdict: PASS (no red case and no blocker; this is not an approval of the change)
cases: executed 126→126, red 0
origin: introduced 1, pre-existing 1, undecided 0
wrote-outside-worktree: the assigned scratch (91M); rebuilt inside pass 1's scratch copy
needs-coordinator: no
```

## Finding

`bounded` ends a stall in the async code under test. A stall inside the renewal's blocking file I/O
(work on tokio's blocking threads) makes the case panic at 120 s and then hangs the process,
because the `#[tokio::test]` runtime's drop waits for its blocking tasks; under default capture
the run prints no panic message. That hang exists at the base `5e00283d` too (panic after 0.5 s,
then a hang to an outer 60 s timeout). What `d240a98f` adds is the `STALL` comment claiming the
bound ends any stall. No code path reaches either; only a regression that blocks inside the plan
or write file I/O does.

## Attacked and not broken

All 14 waits on the code under test are inside `bounded`; `bounded` never returns a value on
timeout, so no case can pass on it (the async stall fails the file in 120.02 s, each failure naming
its wait); every hunk is a pure wrap (clean binary 10 of 10 in 0.26 s; the bound-deleted and
`RefreshRejected` mutants still red at `:323` and `:329`).

```findings
- file: crates/llm-credentials/tests/adversary_codex_renewal.rs
  line: 33
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the STALL comment says the bound ends a stall the code under test never ends itself, but a stall inside spawn_blocking work panics at 120 s and then hangs the process (exit 124 at 200 s) with no output under capture
- file: crates/llm-credentials/tests/adversary_codex_renewal.rs
  line: 104
  category: concurrency
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: a stall in the renewal's blocking file I/O hangs the file because the test runtime's drop waits for its blocking threads, and this reproduces at 5e00283d (panic at :267 after 0.5 s, then a hang to 60 s)
```

## Routing

- `:33`: back to the implementor, comment only.
- `:104`: accepted and recorded here; no story. Only a regression that blocks inside the renewal's
  file I/O reaches it, and ending it needs a hand-built runtime per case or a process abort.
