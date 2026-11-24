# Wave 2026-11-24-openai-decisions-client

Status: **delivered** on the integration branch at `d03e93393`; `story:openai-decisions-client` is
`implemented`. Wave 2 of 3 of the [wave plan of 2026-11-24](trajectory.md). Approval:
`approval-record:wave-2026-11-24-openai-decisions-client` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Base: the head of wave 1,
`cc001ee87` (pull request 94, `main` at `58e27a4bb` plus the rubric judge). Integration branch
`wave/2026-11-24-openai-decisions-client`, tree `llm-1124-dec-int`. Skill: AEP implementing 0.26.0.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-24 after wave 1
closed, the lines naming this wave's story:

```
wave 3
  story:openai-decisions-client (inferred)
collision: story:anthropic-access story:openai-decisions-client checks/conformance/src/decisions.rs (inferred)
collision: story:anthropic-access story:openai-decisions-client crates/llm-core/src/vocabulary.rs (inferred)
collision: story:anthropic-access story:openai-decisions-client spec/domains/catalog.yaml (inferred)
collision: story:anthropic-access story:openai-decisions-client spec/domains/decisions.yaml (inferred)
collision: story:offline-replay-eval story:openai-decisions-client docs/design.md (inferred)
```

N is one. The story's scope is `inferred`; with one unit no pair runs at once. The three properties:
implementable in this tree (no credential, no live call; fixture bodies come from the provider's
published API description), no overlap (one unit), one new package (`llm-openai-decisions`) with the
classifier's third path in `llm-decide`, its conformance adapter and scenarios.

The move to `implemented` needs the acceptance evidence: the six scenarios answered by
`llm-conformance check` and the raised `contracts/baseline.json`.

## Pre-flight

- Free disk on `/`: 34 GB (`df -h`, 2026-11-24). The unit builds one tree; the wave 1 unit tree is
  removed.
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (wave 1's deviation; `story:ess-pin-current-and-local-gate-finds-it`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole `task check` runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:openai-decisions-client` | `impl/openai-decisions-client` | `llm-1124-decisions` | `llm-1124-decisions/target` | `5eb17374` | implementor green at `21f8cc1b` (llm-decide tests 55 to 64, llm-openai-decisions 23, conformance 1055 to 1062); adversary pass 1 red, 12 introduced, 1 red case of 14 (`review-result:adversary-w28-openai-decisions-client-pass-1`); correction round 1 by the same implementor green at `39a86004` (the two crates 57 to 62 cases, no product code changed); adversary pass 2 green, 3 introduced and 1 pre-existing, 0 red cases, 3 guards (`review-result:adversary-w28-openai-decisions-client-pass-2`), committed with one assertion changed by the coordinator at `5eb17374`; merged as `d03e9339` |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w28/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-24-openai-decisions-client` at `d03e93393`, Rust 1.98.0, each step run
alone with its own exit status:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the four clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2166 tests in 5 runs, all passed; the workspace run 1893 passed, 1 skipped, 1 leaky; the runs took 44.1 s of the 120 s bound; slowest 10.8 s (`llm-bedrock::eventstream`); no test of this wave among the ten slowest |
| `task docs` | 0 | the three generated pages current, example blocks match |
| `ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` (ess 0.87.0) |
| `llm-conformance check` | 0 | 1062 of 1062, failed 0, skipped 0, three runs (ess 0.87.0 carried in the tree for the one job) |
| `aep plan artifact validate` | 0 | `valid` |

No step skipped itself.

## Reviews

`review-result:adversary-w28-openai-decisions-client-pass-1` (12 findings, 1 red case) and
`review-result:adversary-w28-openai-decisions-client-pass-2` (4 findings, 0 red cases), each with a
`review_outcome` record `fixed` against the story.

- Pass 1, finding 1: a 200 body the transport reports as cut is `transport`, dispatch `accepted`,
  retriable, where three documents said `protocol`. The coordinator decided that the code stays, as
  for every other client over `llm-http`, and the documents change. The adversary's red case was
  re-pinned to what holds for both framings and renamed; pass 2 read it as strong as the case it
  replaces.
- Pass 1, findings 2 to 4: a fixture that was the same expression as another, nine mutants the suite
  missed (each held by a guard), and usage counters taken over the whole of `u64` where the
  reference bounds them (the reading stays; the design names the bound as not enforced).
- Pass 2: two more mutants with a guard each, one sentence held for one framing only (a guard for
  the chunked framing), and one assertion that could not fail, which the coordinator corrected.
- After the second pass the coordinator read the diff: no assertion was dropped; the one change in
  a pass 1 file other than the re-pin is a helper that gained a request id parameter.

A finding that was there before the wave went to the backlog as a draft, and no wave of this plan
takes it: `story:llm-decide-tests-read-no-clock-and-build-one-binary`. A wall-time case of wave 1
(`adversary_w27_judge_pass2`, 80000 numbers in less than eight times the time of 20000) failed once
in the adversary's run while other builds ran on the host, and passed alone and in the gate.

## Deviations

- The implementor's first run was not test-first throughout: it wrote logic before some tests and
  stubbed the bodies to record the red run. The adversary was told and looked for tests that cannot
  fail; it found two.
- The conformance floor rose by 7, not 6: ess derives one scenario for the outcome of the new
  command `llm.decisions.Decide` beside the six authored ones.
- The story names a draft specification file that is not in the tree; `spec/domains/decisions.yaml`
  was the source.
- The unit branched before the release 0.27.0. The integration branch took `main` at 0.27.0
  (`70755acde`) before the unit merged; the merge moved the unit's changelog lines under
  `## [Unreleased]`, set the new package's lock entry to 0.27.0 and wrote the two conflicting
  generated pages anew.
- The coordinator changed one test assertion itself after the second pass
  (`crates/llm-openai-decisions/tests/client.rs`, the escaped fixture) and did not send it back to
  the implementor.
- The implementor could take no worktree lease (`worktree hook session-start` refused in the
  sub-agent); the tree had one writer at a time by the coordinator's order of dispatch.
- The wave was paused once by the operator session for a host-wide stop; the implementor was stopped
  at the start of correction round 1 and resumed from its start.
- The status page counts the decision endpoint among the capabilities of 0.27.0 until the next
  release moves the heading; the changelog has it under `## [Unreleased]`.

## Cost

Sub-agent usage as the harness reported it at each stop (tokens, tool uses, wall time):

| Agent | Stop | Tokens | Tool uses | Wall time |
| --- | --- | --- | --- | --- |
| implementor | first green | 206,679 | 250 | 7,298 s |
| implementor | correction round 1 | 200,050 | 121 | 638 s |
| adversary | pass 1 | 117,606 | 112 | 4,863 s |
| adversary | pass 2 | 232,580 | 54 | 603 s |

Executed cases: `llm-decide` 55 before the wave, 69 after; `llm-openai-decisions` 40, new.
Conformance: 1055 to 1062.
