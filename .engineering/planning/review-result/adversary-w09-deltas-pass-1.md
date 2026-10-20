---
format: aep.planning-md/3
id: review-result:adversary-w09-deltas-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:text-deltas-are-final, pass 1: three introduced, one pre-existing'
relations:
- reviews: story:text-deltas-are-final
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 1 on `story:text-deltas-are-final`, head `d52e6476`
(base `0d444964`). The report as returned, paths outside the repository shortened.

```
unit: story:text-deltas-are-final, head d52e6476 plus two untracked test files
verdict: INFEASIBLE
cases: executed 929→936, red 4
origin: introduced 3 / pre-existing 1 / undecided 0
wrote-outside-worktree: 3 paths in the assigned scratch
needs-coordinator: none
```

All four red cases need a stream shape no recorded server sends (a delta or terminal message with
no id, or a server that rewrites its text); a forced tool choice that is then refused is real.

## Cases

`crates/llm-responses/tests/adversary_w09_shown_text.rs` and
`crates/llm-messages/tests/adversary_w09_opening_text.rs`:

- red `deltas_that_name_no_item_do_not_put_two_messages_text_into_the_first`: id-less deltas over
  two messages carry `AB` and `B` with a false `shown-text-kept`.
- red `a_message_no_delta_showed_keeps_its_own_text_when_ids_cannot_pair`: order pairing gives an
  unshown earlier message the shown text; the answer is carried twice.
- red `two_messages_shown_back_to_back_relay_and_close_on_the_turn`: the Responses ingress encoder
  joins two back-to-back messages and `close` refuses (pre-existing; `story:ingress-block-boundaries`).
- red `a_refused_turn_does_not_tell_the_caller_its_shown_text_was_kept`: the warning reaches the
  sink before `validate_for` refuses a forced-tool turn.
- green: id pairing with several messages, an unshown message beside shown text, a reasoning delta
  inside a message, multi-byte and decomposed Unicode; Messages opening text shown exactly once and
  the relay closing on the turn, also after a cut.

Suite: 932 passed, 4 failed (all in `adversary_w09_shown_text`), 3 ignored, exit 101. Clippy and
conformance (877/877) exit 0.

## Attacked and not broken

`contracts/schema` and `contracts/suite.json` regenerated with ess 0.60.0 are byte-identical to the
committed files (204 `source_digest` values equal); R36 and the Codex backend case still produce the
turn; Chat text comes only from deltas; text shown before a cut stays shown; the renamed scenario
and the three protocol pages match the code except the "by order" rule behind F2.

```findings
- file: crates/llm-responses/src/stream.rs
  line: 280
  category: boundary
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: deltas naming no item are joined into one shown text across output indexes and written into the first message, so two id-less messages come back as AB and B with a false shown-text-kept
- file: crates/llm-responses/src/stream.rs
  line: 339
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: order pairing ignores output_index, so an unshown message before the shown one takes the shown text, the answer is carried twice and a warning is raised
- file: crates/llm-responses/src/ingress.rs
  line: 488
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: two messages shown back to back relay as one message and close refuses the turn, contrary to design decision 2.4, which the unit's relay test sidesteps with a reasoning delta
- file: crates/llm-responses/src/stream.rs
  line: 455
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: shown-text-kept reaches the sink before validate_for, so a refused forced-tool turn still tells the caller its shown text was kept
```
