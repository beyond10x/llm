---
format: aep.planning-md/3
id: review-result:adversary-w09-deltas-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:text-deltas-are-final, pass 2: pass-1 fixed, three introduced'
relations:
- reviews: story:text-deltas-are-final
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 2 on `story:text-deltas-are-final`, head `61f80713`
(correction round 1 on `d52e6476`). The report as returned, paths outside the repository shortened.

```
unit: story:text-deltas-are-final, head 61f80713 plus one untracked test file
verdict: INFEASIBLE
cases: executed 936→941, red 3
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: 3 paths in the assigned scratch
needs-coordinator: none
```

The correction fixes all four pass-1 findings (the pass-1 file runs 5 of 5 green). Three new cases
in `crates/llm-responses/tests/adversary_w09_pass2.rs` are red; none matches a recorded server
stream. G1 is a regression the correction introduced.

## Cases

- red `an_index_the_deltas_and_the_terminal_output_disagree_on_does_not_carry_the_answer_twice`:
  deltas at `output_index` 0, the message at position 1 of the terminal `output` with one id
  missing; the turn carries the answer twice (Harness's emulator uses these positions,
  `fake_responses.py:120-146`, and is saved there by matching ids).
- red `deltas_naming_neither_item_nor_index_do_not_put_two_messages_text_into_the_first`:
  coordinate-less deltas over two messages come back `AB`, `B` with a false warning (the pinned
  `openai-responses/2026-08-31.1` wire has this delta shape, with one message).
- red `a_message_no_delta_showed_keeps_its_text_when_the_deltas_name_no_index`: deltas naming an
  item but no index, messages naming no id; the order pass overwrites the unshown message.
- green: the streamed route pairs by the `output_item.done` index; different ids and consistent
  different indexes never pair; Harness's coordinate-less wire with one message works; the client
  emits `shown-text-kept` after every delta and before `advisory-omitted`.

Suite: 938 passed, 3 failed (all in `adversary_w09_pass2`), 3 ignored, exit 101; clippy and
conformance (879/879) exit 0. `contracts/schema` and `contracts/suite.json` regenerate
byte-identical.

```findings
- file: crates/llm-responses/src/stream.rs
  line: 196
  category: boundary
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: when the deltas' output_index differs from the message's position in the terminal output and one id is missing, the index veto keeps the shown text beside the server's message and the turn carries the answer twice, where base and d52e6476 carried it once
- file: crates/llm-responses/src/stream.rs
  line: 358
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: deltas naming neither item_id nor output_index, the shape of the pinned openai-responses/2026-08-31.1 wire, still join into one shown text, so two messages come back as AB and B with a false shown-text-kept
- file: docs/responses.md
  line: 328
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the order pass at stream.rs:204 overwrites a message no delta showed when deltas name an item but no index and messages name no id, contrary to keeps the server's text wherever it sits
```
