# Wave 2026-11-25-tests-without-a-clock

Status: **open**. Wave 9 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-tests-without-a-clock` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Opened on the plan commit
`183126a4d`, which sits on `main` `831c21d95` (LLM 0.32.0 with wave 8 merged), while the release of
0.33.0 and the plan's pull request are in review; the wave takes `main` before its gate. Integration
branch `wave/2026-11-25-tests-without-a-clock`, tree `llm-1125-clk-int`. Skill: AEP implementing
0.26.0.

Consumer gap moved: none. Both stories are test defects found by adversaries of earlier waves: a
case of `llm-decide` that compares wall times failed once under load, and a case of `llm-messages`
that cancels after a fixed 100 ms failed once under load.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `183126a4d`:

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

N is two. The verb names no collision between the two stories of this wave.
`story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification`.
`story:docs-ux-layout-checks-from-ux-browser` is wave 7 and touches no file of this wave. The three
properties: implementable in this tree (no credential, no live call), no overlap of files between
the units or with the open wave, and both land before wave 10 moves the files they edit.

The move to `implemented` needs the acceptance evidence. For `llm-decide`: no test under
`crates/llm-decide/tests/` reads a clock; each of the two cases fails on a reader whose work grows
with the square of the length and passes 50 runs in a row beside a build; the crate links one
integration-test executable and executes the same number of cases. For `llm-messages`: the
cancellation case holds no `sleep`, keeps its three assertions, and passes 100 runs beside a build;
the other cases that wait a fixed time are listed, each changed or named with its reason.

## Units

| Unit | Story | Branch | Tree |
| --- | --- | --- | --- |
| 1 | `story:llm-decide-tests-read-no-clock-and-build-one-binary` | `impl/llm-decide-no-clock` | `llm-1125-clk-dec` |
| 2 | `story:messages-cancellation-case-waits-on-a-signal` | `impl/messages-cancel-signal` | `llm-1125-clk-msg` |

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| `llm-decide`: where the two cases live | unit tests beside the reader in `crates/llm-decide/src/answer.rs`; a counter under `#[cfg(test)]` cannot be read from `tests/`, and the public API does not change |
| `llm-decide`: what is counted | the reader's work (`answer::whole` with `integers_are_exact`), as the story's Outcome says |
| `llm-decide`: "the same number of cases" | the number `cargo test -p llm-decide` executes over the whole package |
| `llm-decide`: the layout | `crates/llm-decide/tests/main/main.rs` with the five files as modules beside it, as `checks/xtask/tests/gate/main.rs` |
| `llm-messages`: which signal | the server's signal after it wrote the head and a signal the sink gives when `emit` is entered; the server's alone does not prove the client read the head |
| `llm-messages`: the third acceptance line | the scoper's list goes into the story's close; `the_turn_deadline_bounds_a_sink_that_never_accepts` is changed or named with its reason |
| Documents and CHANGELOG | none: no behaviour a caller sees changes. The wave has no release of its own; its commits ship with the next release that has an entry |

## Pre-flight

- 86 GB free on `/`; the shared compile cache; builds on the workstation's pool.
- Two unit trees and the integration tree, each with its own `target/`.
- No credential, no live call, no network request in either unit.
