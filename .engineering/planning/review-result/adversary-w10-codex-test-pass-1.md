---
format: aep.planning-md/3
id: review-result:adversary-w10-codex-test-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w10 adversary, story:codex-renewal-test-load-independent, pass 1: no bound before the exchange starts'
relations:
- reviews: story:codex-renewal-test-load-independent
revision: 1
---
## Pass

Wave 2026-10-20-w10, `aep:adversary` pass 1 on `story:codex-renewal-test-load-independent`, head
`11d527eb` (base `5e00283d`). The report as returned, paths outside the repository shortened.

```
unit: story:codex-renewal-test-load-independent at 11d527eb
verdict: PASS (no red case, no blocker; this is not an approval of the change)
cases: executed n/a→n/a, red 0 (no case added; the gate was killed by the disk watch before printing any test output)
origin: introduced 1, pre-existing 0, undecided 0
wrote-outside-worktree: the assigned scratch, about 0.7G
needs-coordinator: the gate has not run; the wave's disk watch killed it at 10:15:05 (23G free, floor 25G)
```

## Finding

The case no longer has a time bound of its own before the renewal exchange starts. With a stall
injected after `renewal.rs:591` in a scratch copy, the new case ran until an outer `timeout 60`
killed it (exit 124, 60.07 s); the base case failed in 0.50 s. No code path reaches it today; only a
regression that stalls before the exchange starts does, and CI's job sets no `timeout-minutes`. The
comment at `:281-282` claims the 30 s exchange bound ends a resolve that never reaches the endpoint,
which holds only once the exchange has started.

## Attacked and not broken

- One-CPU starvation (16 busy loops pinned to one CPU, the case at nice 19): base red 5 of 5 with the
  w09 message; new case green 20 of 20, load average 52.8 to 68.6. 64 loops: green 3 of 3, load
  average up to 116.3.
- Mutants in `renewal.rs` (delete the `RefreshUncertain` bound; record `RefreshRejected`; flip the
  comparison at `:582`): each red.
- A resend cannot pass: the endpoint counts before it answers. No lost wakeup in `reached`/`Notify`.
  The drop happens in the original window (grant read, no answer). The other nine cases use no fixed
  short timeout.

```findings
- file: crates/llm-credentials/tests/adversary_codex_renewal.rs
  line: 281
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the case has no bound of its own before the exchange starts, so a resolve that stalls before sending hangs it past 60 s (CI default 360 min) where the base case failed in 0.5 s, and the comment at :281-282 says the 30 s exchange bound ends such a resolve
```

## Routing

Back to the implementor: an outer bound well above the 30 s exchange bound, with a message, and the
comment corrected.
