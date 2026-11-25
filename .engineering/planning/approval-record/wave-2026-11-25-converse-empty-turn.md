---
format: aep.planning-md/3
id: approval-record:wave-2026-11-25-converse-empty-turn
kind: approval-record
status: accepted
title: Wave 2026-11-25-converse-empty-turn authorised by the owner's order to run the next wave from the active stories
tags:
- non-interactive
relations:
- decides: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T10:05:17Z", actor: "human:operator", revision: 2}
---
## Decision

Wave 2026-11-25-converse-empty-turn runs: wave 8 of the wave plan in `docs/plan/trajectory.md`, one
unit, `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`.

## Source

The owner's order of 2026-11-24, relayed by the operator session that acts with the owner's
permissions: "run the next wave from the active stories", "one after the other", each merged through
that session. The story was accepted on 2026-11-25
(`approval-record:backlog-2026-11-25-bedrock-converse-accepted`). No operator turn precedes the
dispatch, so the stage-1 stop is recorded here and the wave runs.

## The wave

`aep plan artifact waves --kind story --status active` (aep 0.80.0) on 2026-11-25, at `24390c662`
(LLM 0.32.0), before the Scope of this story was written:

```
wave 1
  story:anthropic-access (inferred)
  story:docs-ux-layout-checks-from-ux-browser (inferred)
unassessed: story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason
1 wave(s), 0 collision(s), 1 unassessed
```

Check path: the verb, then a scoper's read of the story (the Scope section and the scope entries
this change writes). `story:anthropic-access` stays out under
`credential-blocker:anthropic-live-qualification`. `story:docs-ux-layout-checks-from-ux-browser` is
wave 7, open on its own branch and in its adversary's second pass: its files are `ux/`,
`docs-ux.yml`, `Taskfile.yml`, `checks/conformance/src/ess.rs` and `checks/xtask/tests/gate/`; this
wave's are `crates/llm-bedrock`, `spec/domains/bedrock.yaml`, `contracts/` and the retry documents.
Both add a row or a sentence to `AGENTS.md` and a line to `CHANGELOG.md`; the wave that merges second
takes `main` first. The three properties: implementable in this tree (no credential, no live call,
recorded frames only), no overlap of code with the open wave, and it repeats for Converse the rule
wave 2026-11-25-messages-empty-turn delivered for Messages. Pre-flight: 44 GB free on `/`, shared
compile cache, one unit tree, builds on the pool's build node.

## What the coordinator settled before dispatch

- The story's "Open" line: `guardrail_intervened` and `content_filtered` with no item map to
  `refused`, final, with no cause named, as Messages `refusal` does
  (`crates/llm-messages/src/decode.rs`). Converse has no `refusal` reason; without rows of their own
  the two fall to `unavailable`, retriable, and the same request is sent again to the same guardrail.
- `model_context_window_exceeded` with no item is `too-large`, final. Every other unmodelled reason
  with no item (`stop_sequence`, a name in no list) is `unavailable`, retriable.
- "Retriable" in the acceptance holds for the stream. A whole Converse answer is final on every
  row, as `crates/llm-bedrock/src/client.rs` makes every whole answer and as `AGENTS.md`,
  `docs/design.md` and `docs/bedrock.md` say today.
- Usage is attached to the error and final, with dispatch `accepted`, as in Messages.
- Tests go into the existing files of `crates/llm-bedrock/tests/`; no new test binary.
- The specification changes first: the "Failures" comment of `spec/domains/bedrock.yaml`, then
  scenarios under `contracts/bedrock/scenarios/`, then the code.

## Commits this authorises

The opening plan commit, one unit commit and its corrections, the merges into
`wave/2026-11-25-converse-empty-turn`, the closing store commit, and the merge of that branch into
`main` through one pull request. Nothing else.
