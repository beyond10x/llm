# Wave 2026-11-25-converse-empty-turn

Status: **open**. Wave 8 of the [wave plan](trajectory.md). Approval:
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
| 1 | `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason` | `impl/converse-empty-turn` | `llm-1125-cnv` | `llm-1125-cnv/target` | | dispatched |

Each tree is a managed worktree under the worktree tool's tree root (`llm/<tree>`). Scratch lives
outside the repository in the coordinator's scratch directory, `w35/u1`.

## Commits this wave makes

The opening plan commit, one unit commit and its corrections, the merges into the integration branch,
the closing store commit, and the merge of the integration branch into `main` through one pull
request. A release is a separate change after the wave closes.
