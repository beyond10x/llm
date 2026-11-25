# Wave 2026-11-25-replay-last-case-scenarios

Status: **open**. Wave 11 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-replay-last-case-scenarios` (the owner's order of 2026-11-24 to run
the next wave from the active stories, relayed by the operator session). Opened on wave 10's opening
commit `94db8e841` with `main` at LLM 0.33.0 merged; the wave takes `main` again before its gate and
merges after waves 9 and 10. Integration branch `wave/2026-11-25-replay-last-case-scenarios`, tree
`llm-1125-rpl-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: none. Outcome O6: two stops of a replay that only Rust tests held become
behaviour the conformance check answers.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at the wave's
base:

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

N is one story and one unit. The verb puts the story alone in its wave. It shares no test file with
waves 9 and 10; all three change `AGENTS.md`, this one its line on the conformance floor.

The move to `implemented` needs the acceptance evidence: the two scenarios are listed in
`contracts/ess-inputs.yaml`; `llm-conformance check` answers 1092 scenarios and skips none; each
scenario fails on a mutant that reads the stop only on a later case's refusal.

## Units

| Unit | Story | Branch | Tree |
| --- | --- | --- | --- |
| 1 | `story:replay-stops-at-the-last-case-have-scenarios` | `impl/replay-last-case-scenarios` | `llm-1125-rpl-u1` |

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| Does a file under `spec/`, `crates/` or `checks/conformance/src/` change? | No. The target reads every member the two scenarios use. A scenario that needs more stops the unit. |
| What makes a scenario fail on the mutant? | It asserts every case sent and none `not-sent` inside the run it reads. |
| Where is the mutant built? | In a managed worktree of its own; it is never committed. |
| CHANGELOG | One line under `### Added` that names the floor 1092 (was 1090). No release of the wave's own. |
