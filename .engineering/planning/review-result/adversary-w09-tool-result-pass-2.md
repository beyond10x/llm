---
format: aep.planning-md/3
id: review-result:adversary-w09-tool-result-pass-2
kind: review-result
status: active
title: 'Wave 2026-10-20-w09 adversary, story:tool-result-rejected-or-failed, pass 2: pass-1 fixed, three notes'
relations:
- reviews: story:tool-result-rejected-or-failed
revision: 1
---
## Pass

Wave 2026-10-20-w09, `aep:adversary` pass 2 on `story:tool-result-rejected-or-failed`, head
`ce7892bf` (correction round 1 on `29f5bc84`). The report as returned, paths outside the repository
shortened.

```
unit: story:tool-result-rejected-or-failed, head ce7892bf plus two uncommitted test files
verdict: CONFIRMED (3 findings, all notes, none blocks the merge)
cases: executed 349→351, red 2
origin: introduced 3 / pre-existing 0 / undecided 0
wrote-outside-worktree: the assigned scratch
needs-coordinator: none
```

## Pass-1 findings as corrected

Read order fixed; depth documented and pinned at 126/127 levels for Chat (a successful Responses
output past the limit is the remainder, finding 1); a repeated top-level key fixed (a key repeated
inside `error` is the remainder, finding 2); the design.md cell fixed; the large-payload parse
guarded.

## Cases (both red)

- `crates/llm-responses/tests/adversary_w09_tool_result_depth.rs`: a successful Responses output
  nested 127 levels reads back as the envelope's text, though the docs list only failed outputs.
- `crates/llm-chat/tests/adversary_tool_result_2.rs`: `{"error":{"code":404,"code":500},"ok":false}`
  reads back failed with the last value, though `spec/domains/chat.yaml:95` says any repeated key
  leaves a successful string.

## Attacked and not broken

Arrays, NBSP, U+3000 or a BOM before `{`, numeric keys, missing or mistyped fields, `{}` and
trailing text stay successful strings byte for byte; the escaped key `error`, whitespace and
nested failure objects read back failed; the 21-byte frame and the bound behave as documented;
five mutants each turn a committed test red; contracts regenerate identical.

```findings
- file: docs/design.md
  line: 102
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: a successful Responses output nested more than 126 levels also reads back as the envelope's text, but design.md and responses.md:156-158 list only failed outputs as the exception to coming back unchanged
- file: spec/domains/chat.yaml
  line: 95
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the spec says content with a repeated key stays a successful string, but a key repeated inside the error value reads back failed with the last value kept
- file: crates/llm-chat/tests/adversary_tool_result.rs
  line: 125
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the doc comments at :125 and :141 describe the read-order and repeated-key defects as present although ce7892bf fixed them and the cases pass
```
