---
format: aep.planning-md/3
id: review-result:adversary-w30-messages-empty-turn-pass-2
kind: review-result
status: active
title: 'Adversary, pass 2 (story:messages-empty-turn-with-an-unmodelled-stop-reason, wave 2026-11-25-messages-empty-turn): 3 introduced, 2 red cases'
relations:
- reviews: story:messages-empty-turn-with-an-unmodelled-stop-reason
revision: 1
---
Recorded by the coordinator as the adversary returned it, with one change: the three entries of the
findings block said `severity: low`, which the store does not read, and say `note` here. Covers head
`1dd627ec3` of `impl/messages-empty-turn` (correction round 1).

unit: story:messages-empty-turn-with-an-unmodelled-stop-reason
verdict: red
cases: executed 375→379, red 2
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: session scratch `w30/u1/adv2/` only (scripts, logs, copies `base/` and `mut/`)
needs-coordinator: yes

`git --no-pager diff --stat` at head `1dd627ec3` (nothing staged, nothing committed):

```
 crates/llm-messages/tests/retry_class.rs | 131 +++++++++++++++++++++++++++++++
 crates/llm-messages/tests/streaming.rs   | 112 ++++++++++++++++++++++++++
 2 files changed, 243 insertions(+)
```

Cases added (4; `rustfmt --edition 2024 --check` exit 0 on both files):

- RED `crates/llm-messages/tests/retry_class.rs:868`
  `adversary_w30b_a_cut_after_a_delta_without_content_is_final_only_with_its_costs_shown`
- RED `crates/llm-messages/tests/retry_class.rs:911`
  `adversary_w30b_an_overload_after_a_delta_without_content_is_final_only_with_its_costs_shown`
- green guard `crates/llm-messages/tests/streaming.rs:2455`
  `adversary_w30b_a_delta_after_content_shows_its_cost_with_that_delta`
- green guard `crates/llm-messages/tests/streaming.rs:2506`
  `adversary_w30b_a_modelled_reason_without_content_shows_the_held_costs_at_its_delta`

Red output, the two cases run alone (`case-a.log`):

```
panicked at crates/llm-messages/tests/retry_class.rs:929:5:
final for having answered, and the reported costs were dropped:
overloaded_error after Some("pause_turn"): Unavailable retriable false shown []
overloaded_error after None: Unavailable retriable false shown []
panicked at crates/llm-messages/tests/retry_class.rs:901:5:
final for having answered, and the reported costs were dropped:
InsideAnEvent after Some("pause_turn"): Protocol retriable false shown []
InsideAChunk after Some("pause_turn"): Transport retriable false shown []
a clean end after Some("pause_turn"): Protocol retriable false shown []
InsideAnEvent after Some("refusal"): Protocol retriable false shown []
InsideAChunk after Some("refusal"): Transport retriable false shown []
a clean end after Some("refusal"): Protocol retriable false shown []
InsideAnEvent after None: Protocol retriable false shown []
InsideAChunk after None: Transport retriable false shown []
a clean end after None: Protocol retriable false shown []
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 16 filtered out
```

The `end_turn` control rows in both cases pass. Same two cases on the base copy: 2 passed
(`case-a-on-base.log`). On head with pass 1's `decode.rs`: 2 passed (`case-a-on-pass1.log`). So
correction round 1 introduced it.

Suite, in the worktree, `RUSTUP_TOOLCHAIN=1.98.0 cargo test --locked --no-fail-fast` for
`-p llm-messages`, `-p llm-bedrock --test client`, `-p llm-models --test relayed_failure`
(`suite.summary`):

- before (`--skip adversary_w30b_`): 337 + 32 + 6 = 375 executed, 0 failed
- after: 341 + 32 + 6 = 379 executed, 2 failed, both the red cases above. The count moved by the 4
  cases added.
- No added test is over 5 s: the `retry_class` binary ran 0.20 s, `streaming` 0.06 s.
- Conformance check not run: no scenario added or changed.

Findings:

1. Final failure with the reported costs dropped. `crates/llm-messages/src/decode.rs:850` holds a
   delta's cost while the turn has no content and no modelled reason; `:417` shows nothing on
   `error`; `crates/llm-messages/src/client.rs:359` makes the failure final because the delta
   answered. A stream with a costed header, a costed `message_delta` after no content (reason
   unmodelled or absent), then a cut, a clean end or `overloaded_error` before `message_stop`, ends
   final with neither cost shown. Base and pass 1 showed both costs at the delta. This contradicts
   `docs/messages.md:176` "A turn that answers loses none". Differential probe (every sequence to
   depth 6 over ten events, 49,760 sequences, base against head, `probe-diff.summary`): 497 sequences
   end in the same final failure with costs shown at base and none at head. Reach: the case drives
   the public client with wire bytes; I found no route observed to send that combination, so severity
   is low. Decision needed, default A: (A) show what is held when a failure is final for having
   answered, which restores the base's output for those 497 sequences; (B) let a delta after no
   content not count as answered, so the failure stays retriable and the costs go with the attempt,
   which changes the base's retry class and can bill twice.
2. Documents contradict the code, by reading; the two guards measure the behaviour.
   `docs/messages.md:304` still says a header cost is "Held until the stream answers (the first
   payload other than `ping` and `message_start`)", and `:305` says a delta's cost is "Shown with
   that delta"; both are false for a delta after no content (guard: `[0,0,3]`). `:303` "Each non-null
   `cost` is reported once" is false for a turn failing by the rule. `docs/bedrock.md:286` says
   "nothing answered" for a case where the definition at `:290` ("any event after the message
   header") says it answered. `docs/implementation-status.md:100` repeats "held until the stream
   answers". `CHANGELOG.md` Unreleased has no line for the moved timing, the costs dropped on
   failure, or the new 4096 bound on held reports.
3. Test gap: five mutants pass the unit's suite (110 passed each, `mutants.summary`), each holding a
   delta's cost longer than the rule allows: `decode.rs:480` `is_empty()` → `true`; `:850` condition
   → `true`; `:483` without `MaxOutputTokens`, without `ToolCalls`, without `EndTurn`. Every existing
   test reads the sink after the whole stream. The two guards kill all five (`followup.summary`:
   failed 1, 2, 1, 1, 1). They are green on head and need keeping.

Attacked and not broken:

- Claim 3, measured: of 16,632 probe sequences whose per-event counts differ, none first differs
  after content, and head never shows more than base. No sequence that succeeds differs in what was
  shown; no sequence differs in `answered`.
- Claim 2, measured: the only changed endings are 82 sequences, all base `ok Incomplete` with no
  items → head rule failure with `final_usage: true` and nothing shown.
- Claims 1 and 4, mutants killed by the unit's suite: `final_usage` false (11 failed), dispatch unset
  (16), `attach` overwriting (11), bound 4097 (1), bound removed (1), stop shows held (5), delta
  shows first (5), delta never holds (5), stop never shows (1), held order reversed (2), header cost
  not held (5).
- Claim 6, mutant killed: whole Bedrock answer keeping the mark fails 2 tests in
  `crates/llm-bedrock/tests/client.rs`.
- `decode.rs:468` `is_empty()` → `true` survives and is equivalent, by reading: every content event
  shows what is held, so nothing is held at `message_stop` when content exists.
- Claim 5, by reading: nothing is emitted while the decoder is empty; the probe shows `shown=[]` for
  all 82 rule failures.
- Claim 7, measured: every test name the four documents cite exists (`names.sh`).
- Not measured: the scenario lane carries no cost and no retry mark, by reading.

```findings
- file: crates/llm-messages/src/decode.rs
  line: 850
  category: behaviour
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: A cost held at a message_delta after no content is dropped when the stream then fails other than at message_stop, and that failure is final for having answered; base and pass 1 showed it. Two red cases in crates/llm-messages/tests/retry_class.rs (:868, :911).
- file: docs/messages.md
  line: 304
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: Rows :303-305 state the old display timing and "reported once"; docs/bedrock.md:286 against :290, docs/implementation-status.md:100 and the CHANGELOG do not carry the moved timing, the dropped costs or the 4096 bound on held reports.
- file: crates/llm-messages/src/decode.rs
  line: 483
  category: test-gap
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: Five mutants that hold a delta's cost with content present or under a modelled reason (:480, :483 three ways, :850) pass the unit's suite; the two guard cases in crates/llm-messages/tests/streaming.rs (:2455, :2506) kill them.
```

## What the coordinator decided

1. Option A: a failure that is final for having answered shows what is held before it returns; only
   the story's own failure drops the costs. Fixed in correction round 2.
2. The documents and the CHANGELOG are corrected in correction round 2.
3. The two guard cases stay.
