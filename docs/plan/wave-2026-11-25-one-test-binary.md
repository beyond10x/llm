# Wave 2026-11-25-one-test-binary

Status: **parked** on 2026-11-25 (section Parked). Wave 10 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-one-test-binary` (the owner's order of 2026-11-24 to run the next
wave from the active stories, relayed by the operator session). Opened on `bdec455e0`, wave 9's
integration branch with the plan commit `1130785f6` merged, while wave 9 waits for its builds; the
wave takes `main` before its gate. Integration branch `wave/2026-11-25-one-test-binary`, tree
`llm-1125-bin-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: none. The story is the organisation's test rule: one integration-test binary
per crate. The workspace holds 247 `tests/*.rs` files in 20 crates, each a binary of its own.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `bdec455e0`:

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

N is one story, in six parts with disjoint paths. The verb puts the story alone in its wave: it
collides with every other active story, because it moves every test file. The trajectory orders it
directly after wave 9, whose two stories edit files this one moves, and ahead of every story that
adds a test file.

The move to `implemented` needs the acceptance evidence: every crate with integration tests links
one test binary, `tests/main/main.rs`, and the three crates with a `harness = false` target two; the
number of tests `cargo nextest list --workspace` reports is unchanged; every test path `AGENTS.md`
cites exists; the build of the test binaries is measured before and after.

## Units

| Part | Crates | Branch | Tree |
| --- | --- | --- | --- |
| 1 | `llm-messages` | `impl/one-binary-messages` | `llm-1125-bin-p1` |
| 2 | `llm-responses` | `impl/one-binary-responses` | `llm-1125-bin-p2` |
| 3 | `llm-chat`, `llm-bedrock` | `impl/one-binary-chat-bedrock` | `llm-1125-bin-p3` |
| 4 | `llm-core`, `llm-http`, `llm-credentials`, `llm-routing`, `llm-providers` | `impl/one-binary-core` | `llm-1125-bin-p4` |
| 5 | `llm-models`, `llm-calls`, `llm-tool-call`, `llm-cli`, `llm-cost`, `llm-docs`, `llm-embed`, `llm-blocking`, `llm-openai-decisions`, `checks/conformance` | `impl/one-binary-models` | `llm-1125-bin-p5` |
| 6 | documents, the generated status page, the gate crate's test, the counts | `impl/one-binary-documents` | `llm-1125-bin-p6` |

Parts 2 and 3 start first. Part 1 starts when wave 9's unit for
`crates/llm-messages/tests/client.rs` is merged. Part 6 starts when the other five are merged.
`crates/llm-decide/tests` is in no part: wave 9 gives it the layout.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The layout | `tests/main/main.rs` with one module per former file beside it; helper modules declared once in `main.rs` and reached as `crate::<helper>`; fixture and data directories stay |
| The three `harness = false` targets | stay binaries of their own (the story's section Open) |
| Crates with one `tests/*.rs` file today | the same layout, so that one rule holds for every crate |
| What keeps the rule | a test in the gate crate: no `tests/*.rs` file beside the three excepted targets (part 6) |
| Tests that run their own executable again | the new module path after `--exact`, and each of the nine sites fails when the child ran no test |
| Tests that change process-wide state | listed per part; kept apart where the move joins them in one process under `cargo test` |
| The build measurement | `cargo nextest run --workspace --no-run` in a tree with an empty `target/`, once before part 1 lands and once after part 6 |
| Documents and CHANGELOG | part 6 updates every cited test path; no CHANGELOG line, and the wave has no release of its own |

## Pre-flight

- 79 GB free on `/`; the shared compile cache; builds on the workstation's pool.
- At most two part trees in work at a time, each with its own `target/`, removed when its part is
  merged.
- No credential, no live call, no network request in any part.

## Parked

The owner stopped every wave of this repository on 2026-11-25 for another goal. The wave is not
closed and nothing of it is merged. State at the stop, every branch pushed:

| Part | State | Head |
| --- | --- | --- |
| 1 | phase 1 committed as work in progress, nothing built or run on it | `impl/one-binary-messages` at `e742c4d22` |
| 2 | committed, its checks green | `impl/one-binary-responses` at `5e9e044db` |
| 3 | committed, its checks green | `impl/one-binary-chat-bedrock` at `b5c0a63fb` |
| 4 | committed, its checks green | `impl/one-binary-core` at `beb5e74f3` |
| 5 | phase 1 committed as work in progress, nothing built or run on it | `impl/one-binary-models` at `9f614c174` |
| 6 | not started | none |

To go on: run phase 2 of parts 1 and 5 (the same tests before and after, by name), commit the result,
merge the five part branches into this branch, take `main` (it holds wave 9, which moved
`crates/llm-messages/tests/client.rs`, a file part 1 moves too), then part 6, one adversary for the
story, the gate and the close. No adversary has read any part. The sentence above that part 1
waits for wave 9 is settled: wave 9 is merged as `c7352c3f7`.
