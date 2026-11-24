# Wave 2026-11-24-offline-replay-eval

Status: **delivered** at `2daa20b11` on `wave/2026-11-24-offline-replay-eval`. Wave 3 of 3 of the
[wave plan of 2026-11-24](trajectory.md). Approval:
`approval-record:wave-2026-11-24-offline-replay-eval` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Opened on the head of wave 2,
`58a108db2`; the integration branch took `main` at 0.28.0 (`af88caecb`) before the unit merged.
Integration branch `wave/2026-11-24-offline-replay-eval`, tree `llm-1124-replay-int`. Skill: AEP
implementing 0.26.0.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-24 after wave 2
closed:

```
wave 1
  story:anthropic-access (inferred)
wave 2
  story:offline-replay-eval (inferred)
collision: story:anthropic-access story:offline-replay-eval checks/conformance/src/calls.rs (inferred)
collision: story:anthropic-access story:offline-replay-eval spec/domains/calls.yaml (inferred)
unassessed: story:classifier-takes-only-a-whole-answer
unassessed: story:docs-ux-layout-checks-from-ux-browser
unassessed: story:ess-pin-current-and-local-gate-finds-it
unassessed: story:messages-empty-turn-with-an-unmodelled-stop-reason
2 wave(s), 2 collision(s), 4 unassessed
```

N is one. `story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification` and
is not in the plan; the four unassessed stories enter the plan after this wave. The story's scope is
`inferred`; with one unit no pair runs at once. The three properties: implementable in this tree (no
credential, no live call: scripted models and loopback servers), no overlap (one unit), and it
builds on what waves 1 and 2 added (the verdict's canonical JSON, the classifier's three paths).

The move to `implemented` needs the acceptance evidence: the six scenarios answered by
`llm-conformance check`, the raised `contracts/baseline.json`, and the `llm-cli` command test
`replay_prints_the_summary_and_exits_nonzero_past_the_cap`.

## Pre-flight

- Free disk on `/`: 52 GB (`df -h`, 2026-11-24). The unit builds one tree; the trees of wave 2 hold
  no build output.
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (`story:ess-pin-current-and-local-gate-finds-it`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit ran its own packages'
tests and the conformance check; the whole gate ran once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:offline-replay-eval` | `impl/offline-replay-eval` | `llm-1124-replay` | `llm-1124-replay/target` | `42705fe19` | merged at `2daa20b11` |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w29/u1`.

The unit's commits: `3d4d57705` (first green), `f12ba18fd` (the adversary's 12 cases of pass 1 and
correction round 1), `42705fe19` (the adversary's 7 cases of pass 2 and two sentences the
coordinator corrected).

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-24-offline-replay-eval` at `2daa20b11`, Rust 1.98.0, each step run alone
with its own exit status:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2262 tests in 5 runs, all passed (the workspace run 1952); the runs took 9.4 s of the 120 s bound; slowest 2.2 s (`llm-models::routes`); `replay_prints_the_summary_and_exits_nonzero_past_the_cap` is among them |
| `task docs` | 0 | the generated pages current, example blocks match |
| `ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` (ess 0.87.0) |
| `llm-conformance check` | 0 | 1068 of 1068, failed 0, skipped 0, three runs (ess 0.87.0 carried in the tree for the one job) |
| `aep plan artifact validate` | 0 | `valid` |

No step skipped itself.

## Reviews

`review-result:adversary-w29-offline-replay-eval-pass-1` (10 findings, 6 red cases) and
`review-result:adversary-w29-offline-replay-eval-pass-2` (6 findings, 0 red cases), each with a
`review_outcome` record `fixed` against the story.

- Pass 1, finding 1 (blocker): admission checks the reserve, so a run with a reserve below a case's
  cost spent 0.11 under a cap of 0.06, and the documents called the cap the most a run may spend.
  The coordinator decided that admission stays, as the story says. The documents now state the bound
  that holds (the cap, less the reserve, plus the cost of the one call that passed it) and tell the
  caller to set the reserve to the most one case can cost. A run stops at the case that takes the
  spend past the cap, also when it is the last (`limit-exceeded`, exit 3). The red case was re-pinned
  to that bound and renamed.
- Pass 1, finding 2: a run whose last case had no known cost reported no stop and exit 0. The code
  changed: such a case stops the run there (`uncertain-spend`, exit 3).
- Pass 1, finding 3: a refusal path showed a member name the caller chose inside a recorded verdict.
  A path now shows only names the corpus format defines.
- Pass 1, findings 4 and 5: two cases could write one record id, and a classification case with a
  long id was never sent. A case id that holds `/` is refused at parse, a rubric id is bounded at 177
  bytes, and the readable or digest request id is chosen on the longest record id the case writes.
- Pass 1, findings 6 to 9: four mutants the suite missed, each held by a guard. Finding 10: a
  recorded verdict is not held to its schema; the reading stays and the documents say so.
- Between the passes the coordinator moved `Routed` from `llm-cli` to `llm-decide`, so the
  conformance target no longer depends on `llm-cli`.
- Pass 2 found no defect a caller can reach: four more mutants with a guard each, and two sentences
  on `spent` after an unknown cost (the CHANGELOG line and the guide), which the coordinator
  corrected. It read the three re-pinned cases of pass 1 as holding more than the ones they replace.
- After the second pass the coordinator read the diff of the test files: no case of pass 1 was
  deleted or ignored.

Filed as drafts, and no wave of this plan takes them:
`story:replay-stops-at-the-last-case-have-scenarios` (pass 2, finding 6: the stop at the last case
is held by Rust tests only, no conformance scenario) and
`story:llm-calls-builds-one-integration-test-binary`.

## Deviations

- The implementor wrote seven tests after their logic (five for `Routed`, one for an embed route,
  one for a call without a price) and said so. The adversary was told and looked there for tests
  that cannot fail.
- `crates/llm-calls/tests/replay.rs` is a seventh integration-test binary of `llm-calls`; the rule
  is one per crate. The story names that path and the crate had no single binary. Filed as a draft.
- Pass 2, finding 6 was introduced by the unit and was not sent back to the implementor: after two
  passes it is filed as a draft story.
- The coordinator corrected two document sentences itself after the second pass (`CHANGELOG.md` and
  `website/docs/guides/use-the-llm-command.md`) and did not send them back to the implementor.
- The story names a draft specification file that is not in the tree; `spec/domains/calls.yaml` was
  the source.
- The unit branched before the release 0.28.0. The integration branch took `main` at 0.28.0
  (`af88caecb`) before the unit merged; the merge moved the unit's changelog lines under
  `## [Unreleased]` and wrote the conflicting generated status page anew.
- The store writes of wave 2 and the approval of this wave first recorded the actor as `human:`
  followed by the login of the workstation account. They were dropped and written again through
  `aep` with the actor `human:operator` (`bc1639483`, `effa8def8`); the transitions carry the
  instant of that replay.
- The implementor and the adversaries took no worktree lease (`worktree hook session-start` was
  refused in the sub-agent); the tree had one writer at a time by the coordinator's order of
  dispatch.
- Not tested, by the implementor's word: the budget engine reads every reservation per command, so
  the ledger's work grows with the square of the cases sent, bounded by 4096 cases.
- The status page counts the replay among the capabilities of 0.28.0 until the next release moves
  the heading; the changelog has it under `## [Unreleased]`.

## Cost

Sub-agent usage as the harness reported it at each stop (tokens, tool uses, wall time):

| Agent | Stop | Tokens | Tool uses | Wall time |
| --- | --- | --- | --- | --- |
| implementor | first green | 223,920 | 265 | 3,822 s |
| implementor | correction round 1 | 47,112 | 141 | 1,705 s |
| adversary | pass 1 | 215,989 | 84 | 775 s |
| adversary | pass 2 | 216,798 | 75 | 789 s |

Executed cases, by each crate's own run in the gate or the unit's report: `llm-calls` 43 before the
wave, 80 after (all features, nextest); `llm-cli` 45 before, 55 after and `llm-decide` 69 before, 78
after (the implementor's `cargo test` counts, before the 7 cases of pass 2). Conformance: 1062 to
1068.
