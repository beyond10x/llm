---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-tests-without-a-clock
kind: approval-record
status: accepted
title: Wave 2026-11-25-tests-without-a-clock authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:llm-decide-tests-read-no-clock-and-build-one-binary
- decides: story:messages-cancellation-case-waits-on-a-signal
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T15:03:32Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-tests-without-a-clock runs: wave 9 of the wave plan in `docs/plan/trajectory.md`,
two units, `story:llm-decide-tests-read-no-clock-and-build-one-binary` and
`story:messages-cancellation-case-waits-on-a-signal`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. Both stories were accepted on 2026-11-25
(`approval-record:backlog-2026-11-25-seven-drafts-accepted`), with the order test defects first. No
operator turn precedes the dispatch, so the stage-1 stop is recorded here and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at the plan
commit `183126a4d` on `main` `831c21d95`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
  story:llm-decide-tests-read-no-clock-and-build-one-binary (inferred)
wave 2
  story:cli-route-explain (inferred)
  story:messages-cancellation-case-waits-on-a-signal
wave 3
  story:replay-stops-at-the-last-case-have-scenarios (inferred)
wave 4
  story:responses-carries-refusal-text (inferred)
wave 5
  story:test-binaries-one-per-crate (inferred)
wave 6
  story:tool-arguments-with-a-repeated-key (inferred)
6 wave(s), 133 collision(s), 0 unassessed
```

Check path: the verb, then the two Scope sections. The verb names no collision between this wave's
two stories: one works in `crates/llm-decide`, the other in `crates/llm-messages/tests/client.rs`.
`story:anthropic-access` stays out under `credential-blocker:anthropic-live-qualification`.
`story:docs-ux-layout-checks-from-ux-browser` is wave 7, open on its own branch; its files are
`ux/`, `docs-ux.yml`, `Taskfile.yml` and `checks/`. The three properties: implementable in this tree
(no credential, no live call), no overlap of files between the two units or with the open wave, and
both must land before wave 10 moves the files they edit. Pre-flight: 86 GB free on `/`, the shared
compile cache, two unit trees, builds on the workstation's pool.

## What the coordinator settled before dispatch

- `llm-decide`, where the two cases live: a counter under `#[cfg(test)]` cannot be read from
  `tests/`, and the story allows no change of the public API. The two cases become unit tests beside
  the reader in `crates/llm-decide/src/answer.rs` and count the reader's work for two lengths.
- `llm-decide`, what is counted: the reader (`answer::whole` with `integers_are_exact`), as the
  story's Outcome says. The comment of the `judge.rs` case that claims checking and writing too is
  not carried over.
- `llm-decide`, "the number of executed cases is the same": the number `cargo test -p llm-decide`
  executes over the whole package, unit tests included.
- `llm-decide`, the layout: `crates/llm-decide/tests/main/main.rs` with the five files as modules
  beside it, the form `checks/xtask/tests/gate/main.rs` has. Cargo finds it without a manifest
  entry, and a module needs no `#[path]`.
- `llm-messages`, the signal: a signal the server gives after it wrote the head does not prove the
  client read it, and a cancel before that is dispatch `Unknown`, the failure the story reports. The
  case waits on the server's signal and on a signal its sink gives when `emit` is entered.
- `llm-messages`, the third acceptance line: the scoper's list goes into the story's close.
  `the_turn_deadline_bounds_a_sink_that_never_accepts` has the same defect; the unit changes it or
  names why a test alone cannot.
- Neither unit changes a line of production behaviour, a document or the CHANGELOG.

## Commits this authorises

The opening plan commit, one commit per unit and its corrections, the merges into
`wave/2026-11-25-tests-without-a-clock`, the closing store commit, and the merge of that branch into
`main` through one pull request. Nothing else.
