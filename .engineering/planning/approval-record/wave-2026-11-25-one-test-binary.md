---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-one-test-binary
kind: approval-record
status: accepted
title: Wave 2026-11-25-one-test-binary authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:test-binaries-one-per-crate
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T15:28:34Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-one-test-binary runs: wave 10 of the wave plan in `docs/plan/trajectory.md`, one
story, `story:test-binaries-one-per-crate`, in six parts with disjoint paths.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. The story was accepted on 2026-11-25
(`approval-record:backlog-2026-11-25-seven-drafts-accepted`), with the order test defects first, and
its open question was answered the same day (the story's section Open: the three `harness = false`
targets stay binaries of their own). No operator turn precedes the dispatch, so the stage-1 stop is
recorded here and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `bdec455e0`
(wave 9's integration branch with the plan commit `1130785f6` merged):

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

Check path: the verb, then the Scope section. The verb puts the story alone in its wave: it
collides with every other active story, because it moves every test file. N is one story. The
trajectory orders it as wave 10, directly after the two test defects of wave 9 and ahead of every
story that adds a test file. The three properties: implementable in this tree (no credential, no
live call); the six parts share no path; and the two stories of wave 9 edit files this story moves
(`crates/llm-decide/tests`, `crates/llm-messages/tests/client.rs`), so the wave is opened on wave 9's
integration branch, the part that holds `crates/llm-messages/tests` starts when wave 9's unit for
that file is merged there, and `crates/llm-decide/tests` is in no part: wave 9 gives it the layout.

The wave is opened while wave 9 waits for its builds: on 2026-11-25 builds on the host were held
for some hours, and moving files needs none. Each part is written first and built when builds are
allowed; wave 9 keeps its place in the order of merges.

## What the coordinator settled before dispatch

- The layout is the story's second form, one directory per crate: `tests/main/main.rs` with one
  module per former file beside it. Helper modules are declared once in `main.rs` and reached as
  `crate::<helper>`. Fixture and data directories do not move.
- The crates that have one `tests/*.rs` file today (`llm-blocking`, `llm-openai-decisions`,
  `checks/conformance`) take the same layout, so that one rule holds for every crate: no
  `tests/*.rs` file beside the three `harness = false` targets.
- That rule gets a test in the gate crate (part 6): a file added later as `tests/<new>.rs` would be
  a second binary again, and nothing else would find it.
- A test that runs its own executable again with `--exact <name>` names the test by its new module
  path, and each of the nine sites fails when the child ran no test. Three of them would pass
  having tested nothing (the story's Scope, "Safety fact").
- The macOS, Windows and docs jobs run `cargo test`, where the tests of one binary share a process.
  Each part lists the tests that change process-wide state and keeps them apart where the move
  joins them (`crates/llm-credentials/tests/adversary_codex.rs` changes the working directory).
- The split, after the scoper's proposal. Part 1: `llm-messages`. Part 2: `llm-responses`. Part 3:
  `llm-chat` and `llm-bedrock`. Part 4: `llm-core`, `llm-http`, `llm-credentials`, `llm-routing`,
  `llm-providers`. Part 5: `llm-models`, `llm-calls`, `llm-tool-call`, `llm-cli`, `llm-cost`,
  `llm-docs`, `llm-embed`, and the three single-file crates. Part 6, after the other five are
  merged: the test paths cited in `AGENTS.md` and the live documents, the generated status page,
  the gate crate's test, the total test count and the second build measurement.
- At most two parts are in work at a time. The adversary reads each part once it is green; the
  whole gate runs once on the integration branch.
- The build of the test binaries is measured before part 1 lands and after part 6
  (`cargo nextest run --workspace --no-run` in a tree with an empty `target/`), once each.
- No CHANGELOG line: the wave changes the layout of tests and no behaviour a caller sees. The wave
  has no release of its own.

## Commits this authorises

The opening plan commit, one commit per part and its corrections, the merges into
`wave/2026-11-25-one-test-binary`, the closing store commit, and the merge of that branch into
`main` through one pull request. Nothing else.
