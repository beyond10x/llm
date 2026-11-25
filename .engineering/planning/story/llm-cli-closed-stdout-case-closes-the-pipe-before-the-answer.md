---
format: aep.planning-md/3
id: story:llm-cli-closed-stdout-case-closes-the-pipe-before-the-answer
kind: story
status: draft
title: The closed-stdout case of llm-cli closes its pipe before the fixture answers
tags:
- from-finding
relations:
- decomposes: epic:contracts
revision: 1
---
## Context

`a_reader_that_closes_stdout_before_the_first_delta_stops_the_turn_with_0` in
`crates/llm-cli/tests/command.rs` starts `llm infer` against a fixture that sends the whole answer
at once, then drops its end of the child's stdout pipe, and expects the child to end with 0 and
``llm: route `haiku`: stdout closed; the turn was stopped`` on stderr. Nothing orders the
two: the child may read the answer and write all of it into the pipe before the test has closed
its end. Then every write succeeds and the child prints the usage line of a finished turn.

The whole gate of wave 2026-11-25-docs-ux-browser saw that once, on the build node at a load of
about 28: the case failed with stderr
`route=haiku protocol=messages input_tokens=12 output_tokens=4 cost=0.000032 USD`, exit 0, 1 of
2131 cases of `cargo nextest run --locked --workspace`. The case alone, 40 times on the build node
at the same commit (`--stress-count 40`): 38 passed, 2 failed, with the same stderr. The wave
changed no file of `crates/llm-cli`; the case is at the wave's base as it is at its head. The order
above is the coordinator's reading of the test; no run showed which write came first.

The sibling case `a_reader_that_closes_stdout_after_a_shown_delta_stops_the_turn_with_0_and_no_panic`
has no such race: its fixture `paused_after_one_delta` sends the rest of the answer only after the
test's signal, which the test gives after it closed the pipe.

## Outcome

The case closes its end of the pipe before the fixture sends the first byte of the answer, so it
holds at every load.

## Acceptance

- The fixture of the case sends no content block before the test's signal, and the test gives the
  signal after it dropped the pipe; the case passes 200 times in a row under
  `cargo nextest run -p llm-cli --test command` with `--stress-count`, and the story's close shows
  the count.
- Every other case of `crates/llm-cli/tests/command.rs` that drops or closes a pipe of a running
  child and then asserts on what the child did is listed in the close, changed the same way or
  named with its reason.
- No case gains a retry, a sleep or a wall-time bound.

## Verification

Targeted: `cargo nextest run -p llm-cli --test command`, the one case repeated, inside the 60 s
bound per test and the 120 s step budget. No live call.
