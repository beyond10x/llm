# Wave 2026-11-25-messages-empty-turn

Status: **delivered** at `1f8f90b5f` on `wave/2026-11-25-messages-empty-turn`; one story
`implemented`. Wave 4 of the [wave plan](trajectory.md), the first of the four stories accepted on
2026-11-24. Approval: `approval-record:wave-2026-11-25-messages-empty-turn` (the owner's order of
2026-11-24 to run the next wave from the active stories, relayed by the operator session). Opened on
the release commit of 0.29.0, `bec3606f9`. Integration branch `wave/2026-11-25-messages-empty-turn`,
tree `llm-1125-empty-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: issue 83 of this repository, a Messages turn that stops for an unmodelled reason
without content and decodes as a success.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, after the four
accepted stories got their Scope sections (one `story-scoper` each, read at `87bc16f16`):

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:classifier-takes-only-a-whole-answer (inferred)
wave 3
  story:ess-pin-current-and-local-gate-finds-it (inferred)
wave 4
  story:messages-empty-turn-with-an-unmodelled-stop-reason (inferred)
4 wave(s), 38 collision(s), 0 unassessed
```

N is one. `story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification` and
is not in the plan. Every pair of the four stories collides on `AGENTS.md` and `CHANGELOG.md`; the
two that add scenarios also collide on `contracts/ess-inputs.yaml`, `contracts/suite.json` and
`contracts/baseline.json`. The order is the coordinator's and is in [trajectory.md](trajectory.md).
The three properties: implementable in this tree (no credential, no live call: recorded streams
only), no overlap (one unit), and it builds on nothing an open wave holds.

The move to `implemented` needs the acceptance evidence: the named tests of `llm-messages` for the
two failures and the two streams after text, the scenarios answered by `llm-conformance check` with
the raised `contracts/baseline.json`, and issue 83 closed with the release that carries the change.

## Decisions for the story's Open section

The story leaves the error codes open. The coordinator settled them before dispatch:

| Stop reason, no content block | Error code | Retried |
| --- | --- | --- |
| `model_context_window_exceeded` | `too-large` | never |
| `refusal` | `refused`, no cause named | never |
| any other unmodelled reason (`pause_turn`, `stop_sequence`, a reason in no list) | `unavailable`, marked retriable | before output only |

A terminal `message_delta` that follows no content block is not output for this failure: no block
opened, so the attempt has not started answering. `docs/design.md` "Provider failures retried before
output" and the retry row of the invariants table in `AGENTS.md` are amended in the same change, as
the story's second acceptance line asks. With a content block, every one of these reasons still
decodes to `StopReason::Incomplete { reason }`.

## Pre-flight

- Free disk on `/`: 52 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (`story:ess-pin-current-and-local-gate-finds-it` is wave 6).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:messages-empty-turn-with-an-unmodelled-stop-reason` | `impl/messages-empty-turn` | `llm-1125-empty` | `llm-1125-empty/target` | `fa7f048ea` | merged at `1f8f90b5f` after two adversary passes and two correction rounds |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w30/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-25-messages-empty-turn` at `1f8f90b5f`, Rust 1.98.0, each step run alone
with its own exit status:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2312 tests in 5 runs, all passed (the workspace run 1998); the runs took 33.3 s of the 120 s bound; slowest 4.7 s (`llm-bedrock::process_credentials`); no test of this wave is among the ten slowest |
| `task docs` | 0 | the generated pages current, example blocks match |
| `ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` (ess 0.87.0) |
| `llm-conformance check` | 0 | `{"total":1075,"passed":1075,"failed":0,"error":0,"unsupported":0,"skipped":0}` three times; `foundation conformance and deterministic ESS projections passed` |
| `aep plan artifact validate` | 0 | `valid` |

No step skipped itself. The integration branch took `main` at 0.29.0 (`904f2a8ed`) before the unit
merged.

The test step took 33.3 s; wave 3's gate took 9.4 s. Measured on the same build node, one job each,
`task test` alone:

| Commit | Workspace run | All five runs |
| --- | --- | --- |
| wave 3's head `2daa20b11`, its gate | 1952 tests in 6.4 s | 9.4 s |
| this wave's head `1f8f90b5f`, the gate | 1998 tests in 27.3 s | 33.3 s |
| this wave's head `1f8f90b5f`, run again | 1998 tests in 28.5 s | 33.0 s |
| this wave's base, `main` at 0.29.0 (`904f2a8ed`), run after it | 1952 tests in 51.8 s | 60.3 s |

The base without the wave's change ran longer than the head with it, so the wave is not the cause;
the time moves with what else runs on the node. No bisect ran. The slowest single test took 4.7 s, 4.6 s
and 5.1 s in the three runs of this wave (`llm-bedrock::process_credentials`) and 2.2 s at wave 3's
gate (`llm-models::routes`); the bound is 60 s.

The story is `implemented` on: the seven scenarios answered by `llm-conformance check` with the
floor at 1075, and the `llm-messages` cases
`a_stream_that_stops_at_the_context_window_without_content_is_too_large_with_its_usage`,
`a_stream_that_stops_for_an_unlisted_reason_without_content_is_a_retriable_provider_failure_with_its_usage`
and `the_context_window_and_an_unlisted_reason_after_a_text_block_stay_incomplete`. Acceptance line 4
(issue 83 closed with the release that carries the change) is done at that release.

## Reviews

| Pass | Covers | Verdict | Cases | Findings | Outcome |
| --- | --- | --- | --- | --- | --- |
| 1 | `4c770f4bd` | red | 7 added, 6 red | 4 introduced, 1 undecided | `review-result:adversary-w30-messages-empty-turn-pass-1`: fixed in correction round 1 (`1dd627ec3`) |
| 2 | `1dd627ec3` | red | 4 added, 2 red | 3 introduced | `review-result:adversary-w30-messages-empty-turn-pass-2`: fixed in correction round 2 (`fa7f048ea`) |

After pass 2 the coordinator read the diff of correction round 2 (`1dd627ec3..fa7f048ea`) and ran
the gate above; no third pass ran.

What the passes changed:

- Pass 1: the failure carried `final_usage` false, so `llm-cost` priced 4000 reported input tokens
  as an unknown quantity; a route that reports `usage.cost` never got the retriable row; a whole
  Bedrock `InvokeModel` answer was retriable against `docs/bedrock.md`; the `refusal` row survived
  two prefix mutants.
- Pass 2: correction round 1 held a reported cost and then dropped it on a failure that is final for
  having answered (a cut, an end without `message_stop`, `overloaded_error` after a costed
  `message_delta` with no content): 497 event sequences of depth 6 showed the costs at the base and
  none at the head. Three documents still stated the old timing; five mutants that held a delta's
  cost too long passed until the two guard cases.

## Deviations

1. The wave order is the coordinator's, not the verb's (see [trajectory.md](trajectory.md)).
2. The story's Open section was settled by the coordinator before dispatch (the table above), not by
   the owner.
3. The implementor's decision that a shown usage-extension warning makes the failure final was
   reversed after pass 1: the cost is held and dropped with the failed turn.
4. The unit changed `crates/llm-bedrock/src/` (a whole `InvokeModel` answer is final; the streamed
   path shows held costs before a final cut) and added two public functions,
   `StreamDecoder::fail` and `StreamDecoder::end`. Neither was in the Scope section.
5. On a stream with no content before its `message_delta`, a `usage-extension` warning is now shown
   later than before (at a delta that names a modelled reason, before later content, or before a
   final failure). The CHANGELOG names it under Changed.
6. The story is `implemented` before its acceptance line 4: the issue closes with the release.
7. The adversaries' mutants and base copies were built in copies under the session's scratch
   directory. The build pool keeps a job whose directory is under a cache directory on the
   workstation, so those jobs did not go to the build node. `AGENTS.md` now says a mutant run builds
   in a managed worktree.
8. The release commit of 0.29.0 reached `main` through a merge of its pull request, not by a
   fast-forward push; the tag sits on the merge commit, whose tree is the release commit's.
   `AGENTS.md` "Release" names that route.

## Backlog filed by this wave

Drafts; accepting one is the owner's act:

- `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`: Bedrock Converse has its own
  copy of the reason mapping and still decodes such a turn as a success.
- `story:messages-cancellation-case-waits-on-a-signal`:
  `cancellation_reaches_a_caller_whose_sink_never_accepts` failed once of 42 runs on a fixed 100 ms
  sleep.

## Cost

As the harness reported each agent run (tokens, tool uses, seconds):

| Agent | Run | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| implementor | first green | 128,933 | 179 | 1,420 |
| adversary | pass 1 | 50,540 | 95 | 954 |
| implementor | correction round 1 | 257,013 | 79 | 786 |
| adversary | pass 2 | 50,375 | 102 | 2,072 |
| implementor | correction round 2 | 193,463 | 106 | 1,305 |

The four `story-scoper` runs before the plan, one per accepted story, in the order they returned:
93,784 tokens (28 tool uses, 157 s), 127,945 (29, 222 s), 60,235 (17, 108 s) and 51,136 (16,
107 s). The coordinator did not record which run scoped which story.
