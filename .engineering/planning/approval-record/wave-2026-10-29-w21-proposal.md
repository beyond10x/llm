---
format: aep.planning-md/3
id: approval-record:wave-2026-10-29-w21-proposal
kind: approval-record
status: draft
title: Wave 2026-10-29-w21 authorised by b10x-chef-orchestrator item D202
relations:
- decides: story:codex-stream-metadata-is-not-replayed
- decides: story:unpublished-tool-call-reaches-an-opted-in-caller
revision: 1
---
## The stop

The wave skill's proposal stop for wave 2026-10-29-w21.

## What authorises passing it

Session `b10x-chef-orchestrator`, item D202 (2026-10-29), under the class-C grant of the
workspace instructions: "an llm wave fixing two defects harness found in live runs, and its
release. Approved under the grant". Harness is the blocked consumer.

Conditions quoted from the item:

1. Defect 1: "the event is not replayed as an input item."
2. Defect 2: "Fix it so an undeclared tool name reaches the caller as a call it can refuse, without
   weakening any other validation; if this changes a documented contract of llm, stop and report
   to me instead of shipping."
3. "do NOT move the ESS pin" (D198).

Condition 2 is met by an opt-in control: the documented default refusal is unchanged.

Covered: one integration branch, one pull request through the bot, its merge, one release and its
verification. Not covered: registry publishing, an ESS pin move, every other story.

## The units

`story:codex-stream-metadata-is-not-replayed` and
`story:unpublished-tool-call-reaches-an-opted-in-caller`, both serving
`vision:portable-model-inference`.
