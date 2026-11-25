# Wave 2026-11-25-converse-empty-turn

Status: **delivered** at `a66287d35` on `wave/2026-11-25-converse-empty-turn`; one story
`implemented`. Wave 8 of the [wave plan](trajectory.md). Approval:
`approval-record:wave-2026-11-25-converse-empty-turn` (the owner's order of 2026-11-24 to run the
next wave from the active stories, relayed by the operator session). Opened on LLM 0.32.0,
`24390c662`, while wave 7 is in its adversary's second pass on its own branch; the wave that merges
second takes `main` first. Integration branch `wave/2026-11-25-converse-empty-turn`, tree
`llm-1125-cnv-int`. Skill: AEP implementing 0.26.0.

Consumer gap moved: none. The story came from a finding of wave 2026-11-25-messages-empty-turn: the
Converse decoder has its own copy of the stop-reason mapping and the same defect.

## Plan

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `24390c662`,
before this story's Scope was written:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
unassessed: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
1 wave(s), 0 collision(s), 1 unassessed
```

N is one. `story:anthropic-access` is held by `credential-blocker:anthropic-live-qualification`.
`story:docs-ux-layout-checks-from-ux-browser` is wave 7 and touches no file of this wave's code. The
three properties: implementable in this tree (no credential, no live call), no overlap of code with
the open wave, and it repeats for Converse the rule the Messages wave delivered.

The move to `implemented` needs the acceptance evidence: a Converse answer, streamed and whole,
ending in `model_context_window_exceeded` with no item returns `too-large` with its usage; one
ending in a reason in no list returns `unavailable` with its usage, retriable on the stream and
bounded by the retry policy; the same answers after a text block return
`StopReason::Incomplete { reason }`; the retry rule of `docs/design.md` and the Bedrock retry row of
`AGENTS.md` name the case.

## Decisions before dispatch

| Question | Decision |
| --- | --- |
| The story's "Open" line | `guardrail_intervened` and `content_filtered` with no item are `refused`, final, no cause named, as Messages `refusal` is. Without rows of their own they are `unavailable`, retriable, and the same request goes to the same guardrail again |
| The other rows | `model_context_window_exceeded` is `too-large`, final; every other unmodelled reason (`stop_sequence`, a name in no list) is `unavailable`, retriable |
| "Retriable" for a whole answer | the stream only. A whole Converse answer is final on every row, as `client.rs` makes every whole answer and the three documents say |
| Usage | attached to the error and final, dispatch `accepted`, as in Messages |
| Where the tests go | the existing files of `crates/llm-bedrock/tests/`; no new test binary |
| Order | the specification first (`spec/domains/bedrock.yaml`, scenarios under `contracts/bedrock/scenarios/`), then the code |

## What the scoper read

- One function builds the outcome for both paths: `Header::outcome` in
  `crates/llm-bedrock/src/converse.rs`, called by `StreamDecoder::finish` and by `read_answer`.
- Converse carries no type for the reason: any bounded name. Matched today: `end_turn`, `tool_use`,
  `max_tokens`.
- Usage is set before the outcome is built on both paths.
- No test today has an unmodelled reason with no item, and none covers a whole Converse answer.
- No file under `spec/` or `contracts/bedrock/` names a Converse stop reason today.

## Pre-flight

- Free disk on `/`: 44 GB (`df -h`, 2026-11-25).
- Compile cache: the host's shared sccache server through `~/.cargo/config.toml`.
- Builds and tests run as jobs of the build pool on its build node.
- An adversary's mutant or base copy builds in a managed worktree (`AGENTS.md`).

## Units

Sub-agents: implementor `aep:implementor`, adversary `aep:adversary`. The unit runs its own packages'
tests and the conformance check; the whole gate runs once on the integration branch.

| Unit | Story | Branch | Tree | Build directory | Head | Stage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason` | `impl/converse-empty-turn` | `llm-1125-cnv` | `llm-1125-cnv/target` | `a6fd191b8` | merged at `a66287d35` after two adversary passes and two corrections |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w35/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.

## Close

The gate on `wave/2026-11-25-converse-empty-turn` at `a66287d35`, Rust 1.98.0, each step run alone
with its own exit status, as a job of the build pool on its build node:

| Step | Exit | What it printed |
| --- | --- | --- |
| `task rust` | 0 | the three `cargo check` lines, `cargo fmt --all --check`, the clippy lines |
| `task test` (`cargo xtask test`) | 0 | 2435 tests in 5 runs (2120, 129, 23, 80, 83), all passed (in the workspace run one marked leaky by the runner and 1 skipped); the runs took 18.3 s of the 120 s bound; slowest 5.2 s (`llm-bedrock::process_credentials callers_after_expiry_wait_for_the_refresh_already_stalled_in_flight`), a test this wave did not touch, as the leaky one (`llm-bedrock::process_credentials a_process_the_command_leaves_holding_its_output_does_not_hold_the_answer`) |
| `task docs` | 0 | the generated pages current, example blocks match |
| `llm-conformance ess specify validate --path spec` | 0 | `llm v1 — 18 file(s), valid` |
| `llm-conformance check` | 0 | `{"total":1090,"passed":1090,"failed":0,"error":0,"unsupported":0,"skipped":0}`; `foundation conformance and deterministic ESS projections passed` |
| `aep plan artifact validate` | 0 | `valid` |

No live Bedrock call was made, by rule: every answer is a recorded frame, a frame built by a test
or one of three whole answers written by hand.

The story is `implemented` on its four acceptance lines:

- `model_context_window_exceeded` with no content block is `too-large` with its usage, streamed and
  whole (`tests/converse.rs`, the two row tables; two scenarios);
- a reason in no list with no content block is `unavailable` with its usage, retriable on the
  stream (one scenario) and final on a whole answer (one scenario). The line's "bounded by the
  retry policy" is met as written in deviation 2;
- the same answers after a text block are `StopReason::Incomplete { reason }` (one scenario), and
  so are answers after an empty text block (two scenarios) and after a kept event (one scenario);
- the retry rule of `docs/design.md` and the Bedrock retry row of `AGENTS.md` name the case.

Counts: `llm-bedrock` 185 → 203 cases, `llm-models` 84 → 85, conformance 1080 → 1090, skips 0. No
new test binary; no added case over 5 s.

## Decisions after dispatch

| Id | Decision | Why |
| --- | --- | --- |
| D24 | Converse counts a content block as the Messages decoder does: an answer that held any content block, an empty text block included, is the incomplete turn; only an answer with no content block and no kept event fails | pass 1 found the first rule wider than Messages: the same answer was a retriable error on Converse and `Ok(Incomplete)` on `InvokeModel` through one client |
| D25 | the two tests that put a Converse client under routing's loop through a test adapter are removed | no code outside the tests does that, so they were evidence for routing's loop only, which the Messages tests hold |
| D26 | the Bedrock guide's failures bullet and the struct doc of `BedrockClient` are corrected in the unit although the finding is pre-existing | the unit changes the fact both sentences describe |
| D29 | the retriable row stays retriable whatever output tokens the answer reports; the documents say that each attempt of a caller that retries may be billed | the Messages decoder retries the same answer after a header that reported a cost, and the story copies that rule |
| D30 | "retry and fallback belong to routing" is corrected in `docs/bedrock.md` and the guide although pre-existing; two older sentences that said an `llm.routes/1` document cannot name Bedrock are corrected with it | the unit's "Who retries" paragraph contradicts the first; `crates/llm-models/src/routes.rs` documents the Bedrock endpoint of a routes document |

## Reviews

| Pass | Covers | Verdict | Cases | Findings | Outcome |
| --- | --- | --- | --- | --- | --- |
| 1 | `af5169a2f` | red | 198 → 200, 2 red | 6 introduced (3 warnings, 3 notes), 1 pre-existing note | `review-result:adversary-w35-converse-empty-turn-pass-1`: fixed in correction 1 (`032c890b9`) |
| 2 | `032c890b9` | red | 201 → 203, 2 red | 3 introduced notes, 1 pre-existing note | `review-result:adversary-w35-converse-empty-turn-pass-2`: fixed in correction 2 (`a6fd191b8`) |

Two passes ran, the most the wave rules allow. The coordinator read the diff of correction 2
(`032c890b9..a6fd191b8`, 15 files): under `crates/llm-bedrock/src` it changes doc comments only,
and the two cases the adversary left keep their input and change their expectation to what the
corrected sentence says. The gate above ran after it.

What the passes found:

- Pass 1: an empty text block followed by a stop for an unmodelled reason was a retriable
  `unavailable` on Converse and an incomplete turn on `InvokeModel`; no test held that a failed
  turn shows no `advisory-omitted` warning; "a counter left out is never zero" was untrue for a
  Converse cache counter; the two adapter tests were no evidence for a bound on a Converse retry.
- Pass 2: every pass-1 finding closed. New: a text block "opened by its start" does not exist on
  this wire (such a start is an unknown block with an opaque item); the retriable row was justified
  by "the model produced nothing" while the failure carries any reported output tokens; two
  conditions of `Header::outcome` were held by the crate's tests and by no scenario; an older
  sentence gave routing a retry it does not make for Bedrock. Nine mutants: each red in the crate's
  tests, two green in the conformance check before correction 2 added their scenarios.

## Deviations

1. The wave opened beside wave 7 although `aep plan artifact waves` reports a collision of the two
   stories on `AGENTS.md` (after this story's Scope was written: `2 wave(s), 2 collision(s), 0
   unassessed`; the other collision is with `story:anthropic-access`, which is held, on
   `spec/domains/bedrock.yaml`). This wave merges first, on LLM 0.32.0; wave 7 takes `main` before its own merge.
2. Acceptance line 2 says "bounded by the retry policy". No code in the repository puts a Converse
   client under the retry policy: a Bedrock route has no catalog entry, `Routes::port` hands out
   the bare client and the `llm` command asks once. The line is met as: the retriable mark reaches
   the caller, the client asks once a turn, and routing refuses a Converse client before any
   request (`crates/llm-models/tests/relayed_failure.rs::the_converse_client_asks_once_a_turn_and_routing_takes_it_under_no_catalog_target`).
   A caller that retries bounds the retry itself and may be billed for each attempt; the documents
   say so.
3. The scope grew by the Bedrock guide, the test data README and `contracts/ess-inputs.yaml`.
4. Ten scenarios where the plan foresaw seven: one from pass 1 (an empty text block on the stream)
   and two from pass 2 (an empty text block in a whole answer, a kept event on the stream).
5. An empty text block is an item on the Messages wire and no item on Converse, so an incomplete
   turn with no item exists on Converse only; `docs/design.md` states the difference.
6. Correction 2 was made by a second implementor run started from a written brief: the first
   run's agent was no longer there to resume.
7. The Messages documents still say "the route produced no content" for the same row
   (`docs/design.md`, `docs/messages.md`, `crates/llm-messages/src/decode.rs`); this wave changed
   the Converse sentences only.

## Backlog

No draft is filed by this wave: both pre-existing findings were sentences of documents the unit
already changed and are corrected in it (D26, D30).

Not filed, because each is a pin move by the rule this repository already has: `ux-browser` 0.9.1
(with `--version`; wave 7 pins 0.8.1) and ESS 0.99.2.

## Cost

As the harness reported each agent run (tokens, tool uses, seconds):

| Agent | Run | Tokens | Tool uses | Seconds |
| --- | --- | --- | --- | --- |
| story-scoper | before the wave opened, read-only | 98,873 | 30 | 169 |
| implementor | first green | 197,025 | 171 | 1,568 |
| adversary | pass 1 | 87,894 | 115 | 1,948 |
| implementor | correction 1 | 134,101 | 89 | 1,036 |
| adversary | pass 2 | 198,533 | 58 | 889 |
| implementor | correction 2 | 233,998 | 119 | 844 |
