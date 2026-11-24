---
format: aep.planning-md/3
id: review-result:adversary-w29-offline-replay-eval-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:offline-replay-eval, wave 2026-11-24-offline-replay-eval): 10 introduced, 6 red cases'
relations:
- reviews: story:offline-replay-eval
revision: 1
---
Recorded by the coordinator as the adversary returned it; nothing is changed. Covers head `3d4d57705`
of `impl/offline-replay-eval`.

unit: story:offline-replay-eval
verdict: red
cases: executed 108→120, red 6
origin: introduced 10, pre-existing 0, undecided 0
wrote-outside-worktree: scratch `adv/` only (10 logs, `adv/mutant/` source copy, 29 MB, its `target/` removed)
needs-coordinator: no

Covers head 3d4d57705 plus my two test files. `git diff --stat`: `crates/llm-calls/tests/replay.rs | 199 +`, `crates/llm-cli/tests/command.rs | 421 +`; 2 files, 620 insertions, both test files. No implementation file touched, no scenario added (conformance not run), `.ess-0.87.0/` never created. I took no explicit worktree lease.

Order: cases written, run alone (`adv/calls-alone.log`, `adv/cli-alone.log`), then the suite. `cargo nextest run --locked -p llm-calls -p llm-cli`: 120 run, 114 passed, 6 failed, exit 100 (`adv/suite-final.log`); with `-E 'not test(adversary_w29)'`: 108 passed (`adv/suite-before.log`). Every new test runs under 0.2 s. `cargo fmt --check` and clippy `-D warnings` on both crates pass with my cases.

**1. The cap does not bound the spend.** `crates/llm-calls/src/replay.rs:862` admits on the reserve only. Measured: cap 0.06, reserve 0.000000001, calls of 0.055: `the run spent 0.11 under a cap of 0.06 (2 cases asked)` (`adversary_w29_a_run_spends_at_most_its_cap_when_a_case_costs_more_than_its_reserve`, red). The first case alone can pass the cap. The guide line 251 says "`--cap` is the most the run may spend"; `Replay::money_cap` (:651) says the same. Reaches: `llm replay` with any reserve under a case's real cost; the guide's example holds 0.001. The story's wording (admission) is met; the documents are not. Fix named, not applied: change the wording, or flag `spent > money_cap` in the summary and exit status.

**2. An unknown cost on the last case is reported as a known spend.** `replay.rs:900`: `stopped` is set only when a later case is refused. Measured: one case, no usage: `left: None, right: Some(UncertainSpend)`, `spent` 0.1 (the reserve). Through the command, a one-case corpus whose call the provider answers with HTTP 400: `left: (Null, Some(0))`, `"spent":"0.0001"`, exit 0 (`adversary_w29_replay_of_one_case_whose_cost_is_not_known_says_uncertain_spend`). CHANGELOG:25 and design.md:739 say such a call "stops the run with `uncertain-spend`". Reaches: any run whose last sent case has no priced usage. Two red cases.

**3. A refusal echoes a member name of a recorded verdict.** `replay.rs:106-111` shows any member name of up to 64 letters, digits, `-`, `_`. Measured: `cases[0].recorded.verdict.verdict-marker-stoat-8841: a member given twice` (red). `replay.rs:68` says "so no text of the corpus is in a refusal", guide line 267 "The summary and stderr carry no text of a case"; the unit's own no-text test counts the verdict member `grounds` as case text. Reaches: stderr of `llm replay` for a corpus with a doubled member or over 64 levels inside a verdict, a schema or tool arguments.

**4. Two cases write one record id.** `crates/llm-cli/src/replay.rs:93` gives a turn the id `<run>/<case>`, :117 a classification `<run>/<case>/classify`. Measured: classification case `a` and turn case `a/classify`: `two calls wrote the record ids ["run-1/a/classify", "run-1/a/classify"]` (red). `llm.calls.CallRecord` has identity `id`. Reaches: nothing found; I built the ids, and `llm replay` drops records.

**5. A classification case with a long id is never sent.** `replay.rs:766` falls back to the digest id only when `<run>/<case>` itself passes 256 bytes. Measured with run `run-1`: a case id of 256 bytes is `same`; one of 245 bytes: `left: (Failed, Some(InvalidRequest), 0)` (red), because `/classify` overflows afterwards. Same shape for a verdict and its rubric id. Reaches: nothing found.

**6-9. Four mutants the unit's 108 tests miss**, all applied at once in `adv/mutant/`: 108 passed (`adv/mutant-suite.log`). Each guard is green on the real code and red on the mutant (`adv/mutant-guards.log`):
- `replay.rs:866` and the settle time without `.max(ledger.now_ms())`: guard `adversary_w29_a_clock_that_steps_back_stops_no_case`, mutant gives `(1, 2, Some(ClockReversed))`.
- `replay.rs:107` `<= 64` to `<= 65`: guard `..._a_member_name_is_shown_up_to_64_bytes_and_not_one_past`.
- `replay.rs:756` not-sent record counted unknown: guard `..._a_record_of_a_call_that_was_not_sent_costs_nothing_and_stops_nothing`, mutant gives `c-2 NotSent`.
- `crates/llm-cli/src/lib.rs:386` without `+ 1`: guard `adversary_w29_replay_refuses_a_corpus_file_one_byte_past_the_bound`; the mutant runs a 16 MiB + 1 file and exits 0.

**10. Judgement, read not run.** `replay.rs:531` takes any recorded verdict; a recorded label outside its set is refused at parse, a recorded verdict its own rubric schema does not hold is not, and such a case reports `differs` on every run.

Attacked and not broken:
- Reader: repeated key after unescaping and nested, cut file, trailing bytes, BOM, bounds on both sides (64 levels, 4096 cases, 16 MiB), unknown format, kind and member, `null`, empty and doubled ids; reading is linear.
- Money: no double on the path; `Amount::parse` refuses sign, exponent, a tenth fractional digit and overflow; reserve zero or above the cap refused; one nanounit past the cap not sent.
- The stop: no later case is sent; the four counts sum to `cases` in every outcome I ran.
- `failed` against `differs`, role `Replay`, kind and `primary_record_id` on every record.
- `shadow.rs`: `pair_id` and `skipped` are a mechanical extraction.
- Conformance adapter calls the real `replay` and `Routed`; it computes no reported fact.
- `llm replay` with a classification and a verdict case end to end on the Messages wire: both `same`, exit 0 (two new green cases; the unit tested turn cases only).

```findings
- file: crates/llm-calls/src/replay.rs
  line: 862
  category: contract-drift
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: admission checks the reserve only, so a run spent 0.11 under a cap of 0.06 while the guide and Replay::money_cap call the cap the most the run may spend
- file: crates/llm-calls/src/replay.rs
  line: 900
  category: boundary
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a run whose last sent case has no known cost reports stopped null, exit 0 and the reserve as spent, against "stops the run with uncertain-spend"
- file: crates/llm-calls/src/replay.rs
  line: 106
  category: contract-drift
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a refusal path shows a member name of a recorded verdict although the documents say no text of the corpus is in a refusal or on stderr
- file: crates/llm-cli/src/replay.rs
  line: 93
  category: property
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a classification case a and a turn case a/classify write the same record id run-1/a/classify; I built the ids and found nobody who uses such ids
- file: crates/llm-calls/src/replay.rs
  line: 766
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a classification case whose request id fits 256 bytes but not with /classify fails invalid-request unsent, while a longer id gets the digest form and is sent; no caller with such ids found
- file: crates/llm-calls/src/replay.rs
  line: 866
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: dropping the max with the ledger's clock passes the unit's suite; a clock that steps back then stops the run with ClockReversed
- file: crates/llm-calls/src/replay.rs
  line: 107
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: the 64-byte bound of a shown member name can move to 65 with the unit's suite green
- file: crates/llm-calls/src/replay.rs
  line: 756
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: counting a not-sent record without cost as unknown spend passes the unit's suite
- file: crates/llm-cli/src/lib.rs
  line: 386
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: reading the corpus file without the one byte past the bound passes the unit's suite, and a file of 16 MiB plus one byte is then replayed
- file: crates/llm-calls/src/replay.rs
  line: 531
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a recorded verdict is not held to its rubric schema at parse, unlike a recorded label, so such a case reports differs on every run
```
