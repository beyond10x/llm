# Wave 2026-11-25-ess-pin

Status: **delivered** at `e64d66e6e` on `wave/2026-11-25-ess-pin`; one story
`implemented`. Wave 6 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-ess-pin` (the owner's order of 2026-11-24 to run the next wave from
the active stories, relayed by the operator session). Opened on the head of wave 5, `857979d5d`,
while pull request 102 waited for its merge; the integration branch took `main` before the unit
merged. Integration branch `wave/2026-11-25-ess-pin`, tree `llm-1125-ess-int`. Skill: AEP
implementing 0.26.0.

Consumer gap moved: none. The story comes from wave 1: the local gate passed the conformance check
only with a hand-carried `ess` 0.87.0, while the workstation and the build node carry a newer one.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `857979d5d`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:ess-pin-current-and-local-gate-finds-it (inferred)
2 wave(s), 6 collision(s), 0 unassessed
```

N is one. The order is the trajectory's. `story:anthropic-access` is held by
`credential-blocker:anthropic-live-qualification`. The three properties: implementable in this tree
(no credential, no live call), no overlap (one unit), and it builds on wave 5's head.

The move to `implemented` needs the acceptance evidence: `task check` on a clean checkout passing
with the `ess` version `gate.yml` names, locally and in CI; with another `ess` version on `PATH` the
conformance step naming both versions and exiting non-zero before it compares a projection;
`contracts/baseline.json` not fallen and no scenario skipped.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The version | ESS 0.99.0, the release marked latest on 2026-11-25 |
| Finds it, or refuses | both: a pinned `ess` under `target/` is used; when it is missing it is fetched there with the digest check `gate.yml` makes; the `ess` on `PATH` is used only when it reports the pinned version; otherwise the step names the version found and the version needed and exits non-zero before a projection is compared |
| `ess specify validate` in `task check` | goes through the same resolution |
| One version in every place | a test holds that `gate.yml`, `checks/conformance/Cargo.toml` and the documents name one ESS version |

## Pre-flight

- Free disk on `/`: 54 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The workstation's `ess` is 0.98.0 and the build node's is 0.99.0 (the conformance step of wave 5's
  gate printed it).
- An adversary's mutant or base copy builds in a managed worktree (`AGENTS.md`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:ess-pin-current-and-local-gate-finds-it` | `impl/ess-pin` | `llm-1125-ess` | `llm-1125-ess/target` | `140108fcd` | merged at `e64d66e6e` after two adversary passes and three corrections |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w32/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-25-ess-pin` at `e64d66e6e`, Rust 1.98.0, each step run alone with its own
exit status, as a job of the build pool on its build node:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2415 tests in 5 runs, all passed (the workspace run 2101); the runs took 9.9 s of the 120 s bound; slowest 4.7 s (`llm-bedrock::process_credentials an_expired_answer_makes_every_caller_wait_for_the_one_run`), a test this wave did not touch |
| `task docs` | 0 | the generated pages current, example blocks match |
| `llm-conformance ess specify validate --path spec` | 0 | `ess 0.99.1`, taken from `PATH` of the build node; `llm v1 — 18 file(s), valid` |
| `llm-conformance check` | 0 | `{"total":1080,"passed":1080,"failed":0,"error":0,"unsupported":0,"skipped":0}` three times; `foundation conformance and deterministic ESS projections passed` |
| `aep plan artifact validate` | 0 | `valid` |

No step skipped itself and no `ess` was carried in by hand. The build node carries `ess` 0.99.1 on
`PATH`, so the gate made no fetch; the fetch of the pinned release from GitHub ran in the
adversary's pass 1 and passed with the pinned digest. The workstation's own `ess` is 0.98.0 and is
not started.

The story is `implemented` on:

- `task check` passing with the `ess` version `gate.yml` names: locally the gate above; in CI the job
  `libraries-and-plan` of the `Gate` run on this wave's pull request, green before the merge;
- with another `ess` version on `PATH` the conformance step naming both versions and exiting
  non-zero before a projection is made:
  `pinned_ess::another_ess_on_path_is_refused_with_both_versions_before_a_projection_is_made` and
  `pinned_ess::validating_the_specification_refuses_another_ess_the_same_way`;
- `contracts/baseline.json` untouched, 1080 of 1080, none skipped; `contracts/suite.json`
  byte-identical under 0.99.1.

## Reviews

| Pass | Covers | Verdict | Cases | Findings | Outcome |
| --- | --- | --- | --- | --- | --- |
| 1 | `552099b35` | red | 49 → 57, 3 red | 5 introduced (2 warnings, 3 notes), 1 pre-existing note | `review-result:adversary-w32-ess-pin-pass-1`: fixed in correction 2 (`28381cbc4`) |
| 2 | `28381cbc4` | red | 64 → 70, 4 red | 6 introduced, each a note | `review-result:adversary-w32-ess-pin-pass-2`: fixed in correction 3 (`140108fcd`) |

Two passes ran, the most the wave rules allow. The coordinator read the diff of correction 3
(`28381cbc4..140108fcd`) and ran the gate above.

What the passes found:

- Pass 1: `llm-conformance ess --help` was answered by the runner; the guard tests stayed green on
  a Taskfile command written as a map, an `ess` in quotes or after `sh -c`, a second step named
  "Install ess" and a version worded "ESS version 0.87.0"; a fetch ended by a signal left its
  staging directory against the words; a place in the store without an `ess` was downloaded over
  and then refused; two sentences of `AGENTS.md` said more than the code does. Fifteen mutants of
  the resolution and the fetch were killed by the unit's own cases.
- Pass 2: every pass-1 finding closed. New: the runner's help still listed `--help` for `ess`; the
  workflow reader read `run:` only; the runner's words at the end of one line exempted an `ess` on
  the next; "no run starts anything in a `.fetch-*` directory" was false for the fetch's own; a
  lossy copy of the arguments and three mutants of the version reader passed the unit's cases.

## Deviations

1. The wave opened on the head of wave 5 before pull request 102 merged, and took `main` at 0.31.0
   before its unit merged.
2. The pin is ESS 0.99.1, not the 0.99.0 the approval record names: 0.99.1 was marked latest while
   the unit ran, and the move was made by the pin-move rule the unit wrote. The rule missed two
   sites on that first use (`website/docs/getting-started.md` and the CHANGELOG line) and names
   them now.
3. CI's step "Specification and plan" runs the same command as `Taskfile.yml`
   (`llm-conformance ess specify validate --path spec`), the coordinator's decision during the
   unit: a workflow step that started `ess` itself would be a second way to run it.
4. `AGENTS.md` and `README.md` left the planning-or-prose path list of `gate.yml`: the gate's test
   phase reads them now, so a pull request that changes only one of them runs the tests.
5. The scope grew past the scoper's section: a resolver of its own (`checks/conformance/src/ess.rs`),
   guard tests in `checks/xtask/tests/gate/`, and two pages of the site.
6. The build node was unreachable from 06:13 to 08:14 CET on 2026-11-25. Correction 2 was written
   without a build and built when the node answered; no build ran outside the pool.
7. The integration branch carries one commit that is not the unit's: `AGENTS.md` "Release" step 3
   says where the release-tag command runs on a host whose pool builds elsewhere (the route that
   tagged 0.31.0).
8. The guard tests read more than the story asked for: every string of `Taskfile.yml` and of a
   workflow step, on the side of flagging a harmless mention. `echo cargo run --locked -p
   llm-conformance -- ess x` on one line still counts as the runner being handed `ess`; a job's own
   `env:`, `container:` and `name` are not read.

## Backlog

Accepted at this close on the operator session's word of 2026-11-25
(`approval-record:backlog-2026-11-25-bedrock-converse-accepted`), `active`, wave 8 of the plan:

- `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`.

No draft was filed by this wave: both passes found nothing that was there before the wave except
one sentence of `AGENTS.md`, which correction 2 fixed.

## Cost

As the harness reported each agent run (tokens, tool uses, seconds):

| Agent | Run | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| implementor | first green | 270,458 | 158 | 2,308 |
| implementor | correction 1 | 132,564 | 83 | 995 |
| adversary | pass 1 | 195,321 | 69 | 4,912 |
| implementor | correction 2, written without a build | 36,247 | 51 | 889 |
| implementor | correction 2, built and gated | 116,104 | 35 | 464 |
| adversary | pass 2 | 169,405 | 62 | 675 |
| implementor | correction 3 | 163,704 | 31 | 329 |
