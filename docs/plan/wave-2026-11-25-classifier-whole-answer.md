# Wave 2026-11-25-classifier-whole-answer

Status: **delivered** at `319cd2394` on `wave/2026-11-25-classifier-whole-answer`; one story
`implemented`. Wave 5 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-classifier-whole-answer` (the owner's order of 2026-11-24 to run
the next wave from the active stories, relayed by the operator session). Opened on the head of
wave 4, `299c4f906`, while pull request 100 waited for its merge; the integration branch took `main`
before the unit merged. Integration branch `wave/2026-11-25-classifier-whole-answer`, tree
`llm-1125-clf-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: none. The story comes from a finding of wave 1's adversary
(`review-result:adversary-w27-rubric-judge-pass-1`): the classifier takes a cut, split or ambiguous
answer as a decision.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `299c4f906`:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
wave 2
  story:classifier-takes-only-a-whole-answer (inferred)
wave 3
  story:ess-pin-current-and-local-gate-finds-it (inferred)
3 wave(s), 16 collision(s), 0 unassessed
```

N is one. The order is the trajectory's. `story:anthropic-access` is held by
`credential-blocker:anthropic-live-qualification`. The three properties: implementable in this tree
(no credential, no live call), no overlap (one unit), and it builds on wave 4's head, which changes
no file of `crates/llm-decide`.

The move to `implemented` needs the acceptance evidence: a test per case on the JSON-schema path
that fails before the change (a repeated `label`, an answer cut at the output limit, an answer in
two text items), a test per case of the class that reaches the forced-tool path, and one reader
serving the classifier and the judge.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The JSON-schema path's reader | `answer::whole`, the judge's; an answer it does not return abstains `OffLabel` |
| The forced-tool path | the call is taken only from a turn that stopped with `ToolCalls`; a call from a turn cut at the output limit or left incomplete abstains `OffLabel`; two calls abstain, with a test |
| A repeated key in tool arguments | not visible to `llm-decide` (the protocol decoders keep the last value); not in this wave: draft `story:tool-arguments-with-a-repeated-key` |
| The CHANGELOG | `### Fixed`, marked breaking: an answer taken as a decision before abstains now |

## Pre-flight

- Free disk on `/`: 63 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- The conformance check runs with ess 0.87.0, the version CI pins, carried into the build job
  (`story:ess-pin-current-and-local-gate-finds-it` is wave 6).
- An adversary's mutant or base copy builds in a managed worktree (`AGENTS.md`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:classifier-takes-only-a-whole-answer` | `impl/classifier-whole-answer` | `llm-1125-clf` | `llm-1125-clf/target` | `2106be960` | merged at `319cd2394` after one adversary pass and one correction round |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w31/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-25-classifier-whole-answer` at `319cd2394`, Rust 1.98.0, each step run
alone with its own exit status:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2334 tests in 5 runs, all passed (the workspace run 2020); the runs took 48.3 s of the 120 s bound; slowest 11.0 s (`llm-models::routes no_alias_free_document_within_the_byte_bound_is_refused_by_the_alias_bound`, 4.8 s at the base of wave 4), a test this wave did not touch; no test of this wave is among the ten slowest |
| `task docs` | 0 | the generated pages current, example blocks match |
| `ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` (ess 0.87.0) |
| `llm-conformance check` | 0 | `{"total":1080,"passed":1080,"failed":0,"error":0,"unsupported":0,"skipped":0}` three times; `foundation conformance and deterministic ESS projections passed` |
| `aep plan artifact validate` | 0 | `valid` |

No step skipped itself. The integration branch took `main` at 0.30.0 (`38d8e979f`) before the unit
merged; the unit's six CHANGELOG lines were moved from the dated 0.30.0 section, where the merge put
them, to `## [Unreleased]`.

The story is `implemented` on: the five scenarios answered by `llm-conformance check` with the floor
at 1080; a test per case on the JSON-schema path that fails at the base
(`an_answer_that_repeats_label_abstains_off_label`,
`an_answer_cut_at_the_output_limit_abstains_off_label`,
`two_text_items_that_are_an_answer_only_when_joined_abstain_off_label`); a test per case that
reaches the forced-tool path (`a_call_of_a_turn_cut_at_the_output_limit_abstains_off_label`,
`a_call_of_a_turn_reported_incomplete_abstains_off_label`,
`two_calls_to_the_forced_tool_abstain_off_label`, `a_refusal_beside_the_one_call_abstains_off_label`);
and one reader, `answer::whole`, called by the classifier and the judge.

## Reviews

| Pass | Covers | Verdict | Cases | Findings | Outcome |
| --- | --- | --- | --- | --- | --- |
| 1 | `6cdaf1655` | green | 5 probe cases, 0 red, withdrawn | 3 introduced, each a note | `review-result:adversary-w31-classifier-whole-answer-pass-1`: fixed in correction round 1 (`2106be960`) |

No second pass ran: pass 1 was green. The coordinator read the diff of correction round 1
(`6cdaf1655..2106be960`) and ran the gate above.

What the pass found:

- 13 mutants of `answer::call`, `answer::whole` and the classifier were killed by the unit's own
  tests. One survived: `call` taking a call of a turn that says it ended normally. The routed call
  refuses that outcome first; a unit test beside the name check holds it now.
- A whole answer beside an empty text item decided before and abstains now. The Messages and
  Responses decoders can emit such a pair; no fixture shows a provider sending it.
- A scripted `tool_calls` entry that is not JSON failed inside the scripted turn and read as a
  provider error of the library. It is refused where the program is read now, for `Classify` and
  `Judge`.

## Deviations

1. The wave opened on the head of wave 4 before pull request 100 merged, and took `main` at 0.30.0
   before its unit merged.
2. The coordinator settled four questions before dispatch (the table above), not the owner.
3. An empty or whitespace-only text item beside a JSON-schema answer stays an abstention, the
   coordinator's decision after pass 1: the judge's reader reads it so and one reader serves both.
   The documents and the CHANGELOG name the case.
4. `crates/llm-decide/src/answer.rs` gained a function (`call`), where the Scope section said "doc
   comment at most".
5. The conformance adapter now refuses `tool_arguments` that are not JSON where the program is
   read. That was so before the wave for the scripted turn only; the correction covers both members
   because one parse reads both.
6. The adversary built its mutants in a managed worktree (`llm-1125-clf-mut`), as `AGENTS.md` says
   since wave 4, not in a copy under the scratch directory.
7. Two wall-clock tests of the judge failed in runs of the implementor and the adversary on a
   loaded build node (the implementor: 3 of 4 package runs; the adversary: 1 run; both passed in the gate above). The unit did not touch them; the draft
   `story:llm-decide-tests-read-no-clock-and-build-one-binary` covers them.

## Backlog filed by this wave

A draft; accepting it is the owner's act:

- `story:tool-arguments-with-a-repeated-key`: tool-call arguments that give a key twice reach a
  caller with the last value chosen; the protocol decoders parse them.

## Cost

As the harness reported each agent run (tokens, tool uses, seconds):

| Agent | Run | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| implementor | first green | 264,580 | 137 | 1,122 |
| adversary | pass 1 | 223,082 | 78 | 984 |
| implementor | correction round 1 | 112,972 | 61 | 473 |
