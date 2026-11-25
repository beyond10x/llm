---
format: aep.planning-md/3
id: story:xtask-list-check-cases-start-no-file-open-for-writing
kind: story
status: draft
title: The list-check cases of the gate start no file that is still open for writing
tags:
- from-finding
relations:
- informed_by: review-result:adversary-w34-docs-ux-browser-pass-1
- decomposes: epic:contracts
revision: 1
---
## Context

`checks/xtask/tests/gate/list.rs` writes a shell script into a temporary directory (`binary`, with
`fs::write` and `fs::set_permissions`) and has `llm-xtask list-check` start it. The cases of the
gate's test binary run on several threads of one process. While one case holds its script open for
writing, another case's `Command` forks; the child holds a copy of that descriptor until its `exec`,
and the kernel refuses to start a file that is open for writing: `Text file busy (os error 26)`.

The adversary of wave 2026-11-25-docs-ux-browser saw
`list::a_binary_listing_its_tests_as_libtest_does_passes` fail once in 4 runs of
`cargo test -p llm-xtask` with that error (pass 1, finding 7). The wave did not touch the file; it is
at the wave's base as it is at its head. The cause above is the coordinator's reading of the code and
was not reproduced.

## Outcome

No case of the gate's test binary starts a file that the same process wrote while another thread
could fork, so the binary passes at every load and thread count.

## Acceptance

- The cases of `checks/xtask/tests/gate/list.rs` pass 200 times in a row under `cargo test -p
  llm-xtask` with the default thread count, and the story's close shows a run that fails without the
  change.
- Every other test of `checks/xtask/tests/gate/` and `checks/conformance/` that writes a file and
  then starts it is listed in the close, changed the same way or named with its reason.
- No case gains a retry, a sleep or a wall-time bound.

## Verification

Targeted: `cargo test -p llm-xtask`, repeated, inside the 60 s bound per test and the 120 s step
budget. No live call.
