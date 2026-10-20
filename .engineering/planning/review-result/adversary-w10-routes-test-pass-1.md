---
format: aep.planning-md/3
id: review-result:adversary-w10-routes-test-pass-1
kind: review-result
status: active
title: 'Wave 2026-10-20-w10 adversary, story:routes-refusal-test-names-its-field, pass 1: route refusals mislocated at the base'
relations:
- reviews: story:routes-refusal-test-names-its-field
revision: 1
---
## Pass

Wave 2026-10-20-w10, `aep:adversary` pass 1 on `story:routes-refusal-test-names-its-field`, head
`ca6f83d0` (base `5e00283d`). The report as returned, paths outside the repository shortened.

```
unit: story:routes-refusal-test-names-its-field at ca6f83d0 (plus untracked crates/llm-models/tests/adversary_w10_routes.rs)
verdict: PASS
cases: executed 36→38 default / 35→38 --all-features, red 2
origin: introduced 1, pre-existing 2, undecided 0
wrote-outside-worktree: the assigned scratch, 1.2G with a source copy and its target
needs-coordinator: the 2 red cases come from a parser defect at the base, outside this unit's scope
```

## Cases

- red `adversary_w10_an_unknown_field_of_a_later_route_is_refused_at_its_own_line`: the unit's
  ROUTE case with one route before it is refused at line 5 (the `routes:` mapping), not line 6.
- red `adversary_w10_an_unknown_field_of_a_block_route_is_refused_at_its_own_line`: a block-style
  route with `region: eu` on line 7 is refused at line 5.
- green on HEAD, red on mutant M6 (`file` feature):
  `adversary_w10_a_relative_home_refusal_names_no_part_of_the_file_reference`.

Mutants in a scratch copy: dropping `deny_unknown_fields` from the document, endpoint, auth and
declared route (M1-M4) each red in both lanes; the compiled-out `env:` refusal echoing the variable
(M5) red in the default lane; M6 (`file()`'s relative-home refusal echoing the expanded path)
survived the suite and is caught only by the new case.

## Attacked and not broken

Each control document differs from its case only by the field text; `single()` hides no case (no
`should_panic`, `catch_unwind` or `#[ignore]` in the crate); an unknown field inside `capabilities`
is already refused.

```findings
- file: crates/llm-models/src/routes.rs
  line: 122
  category: property
  severity: warning
  verdict: CONFIRMED
  origin: pre-existing
  message: an unknown field in a block route or any route after the first is refused at the first line of the routes mapping, not at the field's line, because the untagged RawRoute error has no position and serde_yaml stamps it with the mapping's start
- file: crates/llm-models/tests/routes.rs
  line: 325
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the ROUTE case's line assertion holds only because its single route sits on the first line of the routes mapping, so it cannot detect the mislocated route refusal its doc comment rules out
- file: crates/llm-models/tests/routes.rs
  line: 764
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: a mutant making file()'s relative-home refusal echo the expanded secret path survives the all-features suite; adversary_w10_a_relative_home_refusal_names_no_part_of_the_file_reference catches it
```

## Routing

- `src/routes.rs:122`: filed as `story:routes-refusal-names-a-later-route-field-line` (draft), with
  the two red cases kept for it.
- `tests/routes.rs:325`: back to the implementor; the ROUTE case's doc comment states what its line
  check covers.
- `tests/routes.rs:764`: the adversary's green case is kept in the unit.
