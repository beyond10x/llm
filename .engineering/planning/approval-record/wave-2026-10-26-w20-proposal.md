---
format: aep.planning-md/3
id: approval-record:wave-2026-10-26-w20-proposal
kind: approval-record
status: draft
title: Wave 2026-10-26-w20 and its release, authorised by b10x-chef-orchestrator under the owner's grant
tags:
- non-interactive
relations:
- decides: story:messages-usage-cost
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-26-w20 (`story:messages-usage-cost`), and
the release that follows it.

## What authorises passing it

Session `b10x-chef-orchestrator`, on 2026-10-26, under the owner's grant for class-C items in
repositories under the workspace: "Approved under the grant in ~/beyond10x/CLAUDE.md (class C:
wave, bot commits, PR, merge, release)." The item: "the llm-messages decoder refuses a `cost` field
inside `message_delta.usage` on the anthropic-messages wire. [...] Decide whether `cost` becomes a
typed optional field or an admitted-and-recorded extension; keep the declared-subset rule's intent
(unknown fields still refused) and say which you chose and why in the story."

Procedure named in the same message: "File the story in the AEP store (aep:planning), accept it
under the grant, implement it in a managed worktree (aep:implementing wave mode: implementor +
adversary), merge through the bot route (b10x-gates bot ... per AGENTS.md), then release the next
minor (0.16.0 unless AGENTS.md's versioning says otherwise), verify with [...] verify-release.sh".

The run is non-interactive: the session runs as a sub-agent of the orchestrator and has no operator
turn.

Covered: the commits of wave 2026-10-26-w20 (the open commit, the `impl/messages-usage-cost`
commits, the merge into `wave/2026-10-26-w20`, the coordinator's integration commit, the close
commit), bot pushes of the wave branch, the pull request, the fast-forward of `main`, and the source
release 0.16.0 through `AGENTS.md` § Release as the bot. Not covered: registry publishing, consumer
pin moves (harness, metaharness, loom, takt), any other story.

## Decisions taken inside the item

- `cost` is an admitted-and-recorded extension, not a typed field: its shape is unknown (the trial
  captured the refusal, not the frame), and the neutral usage and observation types cannot take a
  field without breaking consumers that build them by literal. The story's Decision section says
  why in full.
- Recorded as a `usage-extension` warning on the stream, held on `message_start` until the stream
  answers so retry and fallback eligibility do not move; refused on a complete message, which has
  no sink to carry it on.

## The units, and the objective each serves

One unit, `story:messages-usage-cost`, serving `vision:portable-model-inference`. Selection by
reading: one candidate, filed for this item.
