---
format: aep.planning-md/3
id: approval-record:bedrock-followups-2026-10-23
kind: approval-record
status: draft
title: Bedrock follow-ups and llm 0.13.0, approved by b10x-chef-orchestrator under the owner's grant on 2026-10-23
relations:
- decides: story:bedrock-followups
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for three `llm-bedrock` follow-ups (a public Converse
pre-send check, a non-blocking container credential refresh, `anthropic-beta` carried to Bedrock
`InvokeModel`), and the release of llm 0.13.0.

## What authorises passing it

Session `b10x-chef-orchestrator` approved this wave and the 0.13.0 release on 2026-10-23, under the
owner's directed-session grant in the workspace instructions. The owner asked earlier that every
review finding be fixed.

Covered: the commits of wave 2026-10-23-w17 for `story:bedrock-followups` (the open commit, the
`impl/bedrock-followups` commits, the merge into `wave/2026-10-23-w17`, the close commit), and the
source release of llm 0.13.0 through `AGENTS.md` § Release. The coordinating session pushes,
releases and publishes. Not covered: registry publishing, consumer pin moves in other
repositories, and any serving or gateway change (beyond10x/llm-gateway's own work).

## Decisions taken inside the item

- The beta names are an advisory hint, not a control: Messages and `InvokeModel` carry them,
  every other wire omits them and reports the omission, as the prompt-cache hint already is.
- The pre-send check covers both Bedrock wires, since one function on the target is what a
  gateway calls.
