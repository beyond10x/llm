---
format: aep.planning-md/3
id: review-result:adversary-w34-docs-ux-browser-pass-1
kind: review-result
status: active
title: 'Adversary, pass 1 (story:docs-ux-layout-checks-from-ux-browser, wave 2026-11-25-docs-ux-browser): red, 2 warnings and 5 notes, 3 red cases'
relations:
- reviews: story:docs-ux-layout-checks-from-ux-browser
revision: 1
---
Adversary pass 1 on unit `story:docs-ux-layout-checks-from-ux-browser`, head
`bf6991be7f08d9964a68c3a694916d94a2fc6fee`, base `e762f953e5ae7692b99096cdd263964929416558`.

Verdict: red. Cases executed 222 → 229, 3 red. Origin: introduced 6, pre-existing 0, undecided 1
(the coordinator read finding 7 as pre-existing: the unit does not touch
`checks/xtask/tests/gate/list.rs`, and the file is at the base as it is at the head).

1. The workflow guards miss a second `ux-browser`. `steps` and `pin` of
   `checks/xtask/tests/gate/ux_browser_pin.rs` find none of: a second step that downloads the
   asset of another release; a later step that writes `UX_BROWSER=` to `GITHUB_ENV`; the same line
   appended inside the install step. The invariants row and the "Gate" paragraph of `AGENTS.md`
   say no workflow step starts or fetches one of its own. No step does this today.
2. `uxBrowser` (`ux/core/page-checks.ts`) takes a `UX_BROWSER` that names a directory; the
   documents and the refusal say "an absolute path to a file".
3. `pageChecksEnded` parses the exit file as soon as it exists while the main process writes it in
   place; an empty file throws `Unexpected end of JSON input`. A rare red run of the last L2 case.
4. Eight mutants of the start, stop, signal, exit, teardown, clear and path code pass the whole
   step with 100 cases green: the evidence file kept by `clear`, the kill of the pid instead of the
   group, the child not detached, the signal handler removed, the exit handler removed, the
   teardown stop removed, a relative path taken, `pageChecksEnded` answering exit 0 at its
   deadline. The adversary left four guard cases, green at the head and red on each mutant.
5. `ux/findings/UXF-005.md` line 11 points to result files that no longer carry the keyboard
   failure; it is in `ux/.out/check/page-checks.json` now.
6. Step time, one run each on the loaded build node: 45.2 s at 4 workers, 97.1 s at 2 workers,
   100 cases; 91.8 s at 4 workers pinned to 4 shared CPUs with the slowest case 18.6 s. No run
   over 120 s, no case over 60 s.
7. `list::a_binary_listing_its_tests_as_libtest_does_passes`
   (`checks/xtask/tests/gate/list.rs`) failed once in 4 runs of `cargo test -p llm-xtask` with
   `Text file busy (os error 26)`.

Attacked and not broken: the ESS resolver tests are unchanged against the base but for the tool
argument; `llm-conformance ux-browser` writes the path and nothing else to stdout; the body of the
CI install step, run once under `bash -e -o pipefail`, ended 0 with the binary the resolver stores;
both pin readers refuse zero or two lines; the ratchet refuses before it compares; the teardown, an
exit without teardown, SIGTERM and SIGHUP each end `ux-browser` and what it started. A SIGKILL of
the runner leaves `ux-browser` running to its own end, and no document said so.

```findings
- file: checks/xtask/tests/gate/ux_browser_pin.rs
  line: 481
  category: guard-gap
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: steps and pin find neither a second step that downloads another ux-browser release, nor a later step that writes UX_BROWSER to GITHUB_ENV, nor a second hand-over line in the install step; the invariants row says no workflow step fetches or starts one of its own.
- file: ux/core/page-checks.ts
  line: 66
  category: contract
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: uxBrowser accepts UX_BROWSER naming a directory although the documents and the error text say an absolute path to a file.
- file: ux/core/page-checks.ts
  line: 119
  category: race
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: pageChecksEnded parses the exit file as soon as it exists while it is written in place; an empty file throws Unexpected end of JSON input.
- file: ux/tests/global-setup.ts
  line: 27
  category: test-gap
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: Eight mutants of the start, stop, signal, exit, teardown, clear and path code pass the whole step with 100 cases green; four guard cases left by the adversary catch each.
- file: ux/findings/UXF-005.md
  line: 11
  category: document
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: evidence_lines points to result files that no longer carry the keyboard failure; it is in ux/.out/check/page-checks.json now.
- file: ux/playwright.config.ts
  line: 12
  category: budget
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: At the default of 2 workers the step took 97.1 s on the loaded build node against the 120 s bound; 45.2 s at 4 workers; no case over 18.6 s in any run.
- file: checks/xtask/tests/gate/list.rs
  line: 50
  category: flaky-test
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: a_binary_listing_its_tests_as_libtest_does_passes failed once in 4 runs with Text file busy; the unit does not touch this file.
```
