---
format: aep.planning-md/3
id: review-result:adversary-w09-harness-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:harness-cutover-on-llm-0-7, pass 1: six findings, R28 and M27 not covered'
relations:
- reviews: story:harness-cutover-on-llm-0-7
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 1 on `story:harness-cutover-on-llm-0-7`, commit
`62f107fd` (base `787ac412`). The report as returned, paths outside the repository shortened.

```
unit: story:harness-cutover-on-llm-0-7, commit 62f107fd (crates/ at head byte-identical to tag 0.7.0)
verdict: NEEDS-CHANGE
cases: executed 45→45 (cargo gate, from the implementor's logs; 4 checks are scratch scripts outside it), red 4
origin: introduced 2 / pre-existing 4 / undecided 0
wrote-outside-worktree: the assigned scratch, and Atlas's remote-tracking refs from a git fetch
needs-coordinator: R28 and M27 become a status change
```

## Checks (scratch scripts, red when first run)

- `check-retry-parity.sh`: R28 and M27 say `covered`, but llm 0.7.0 marks only
  `server_is_overloaded` retriable on Responses (`crates/llm-responses/src/stream.rs:477`) and sets
  no retry mark on a Messages `stream_error` (`crates/llm-messages/src/decode.rs:750`); Harness
  retried `server_error`, `rate_limit_exceeded`, `overloaded_error` and `api_error`.
- `check-consumers.sh`: llm-gateway pins nine llm crates at 0.7.0 and is not named on the AGENTS.md
  consumer line.
- `check-header-claims.sh`: rows H8, H18, R9 and R15 changed text and the header does not name
  them; H21 calls the server-delay row H27, which is the cancel row (H20 is server delay).
- `check-rerun-is-a-check.sh`: with H6's `sse.rs:149-153` changed to `:12-16` in a scratch clone,
  the re-run still reports `same` and exits 0; the header's "finds all 638 citations on their line"
  re-run cannot fail for a line citation (only the 264 test and 45 symbol citations are checked).

## Attacked and not broken

- 148 row IDs identical at base and head; status counts 139/2/0/7; the row IDs cited by
  `crates/llm-messages/tests/streaming.rs:182` and `crates/llm-responses/tests/wire_parity.rs:1-2`
  name the right rows.
- All 264 test citations have their `fn` on the cited line at 0.7.0 (an independent awk check).
- About 150 llm line citations across about 110 rows, including all 7 changed rows, read by hand:
  each cited line does what its cell says at 0.7.0.
- The bridge API-change section is complete (every crate the bridge uses diffed 0.1.7 to 0.7.0, the
  CHANGELOG read from 0.2.0 to 0.7.0); all 11 `llm_port.rs` line citations are correct at `8989d138`.
- The README and index statements on Harness, Loom and inference agree with Atlas ADRs 0097 and
  0100 and with the consumers' Cargo pins.

```findings
- file: docs/harness-parity.md
  line: 143
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: R28 is `covered` although llm 0.7.0 marks Responses server_error and rate_limit_exceeded final where Harness retried them, so the bridge's loop loses those retries
- file: docs/harness-parity.md
  line: 192
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: M27 is `covered` although llm 0.7.0 sets no retry mark on any Messages stream error, so overloaded_error and api_error, which Harness retried, become final after the cutover
- file: AGENTS.md
  line: 154
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: the consumer line names Loom and inference but not llm-gateway, which pins nine llm crates at 0.7.0; index.md:45 and the handed-back status text repeat the gap
- file: docs/harness-parity.md
  line: 29
  category: mutant
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the claimed re-run "finds all 638 citations on their line" cannot fail for line citations, as a mutated H6 citation still reports `same` with exit 0
- file: docs/harness-parity.md
  line: 36
  category: judgement
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the header says the text of C1, R13 and M15 changed, but H8, H18, R9 and R15 changed text too
- file: docs/harness-parity.md
  line: 70
  category: judgement
  severity: note
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: H21 calls the server-delay cap row H27, which is the cancel row; the server-delay row is H20
```
