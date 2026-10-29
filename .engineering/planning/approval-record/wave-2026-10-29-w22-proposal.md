---
format: aep.planning-md/3
id: approval-record:wave-2026-10-29-w22-proposal
kind: approval-record
status: draft
title: Wave 2026-10-29-w22 authorised by b10x-chef-orchestrator item D208
relations:
- decides: story:saved-codex-metadata-item-on-resume
revision: 1
---
## The stop

The wave skill's proposal stop for wave 2026-10-29-w22, and the choice between the options
`story:saved-codex-metadata-item-on-resume` left open.

## What authorises passing it

Session `b10x-chef-orchestrator`, item D208 (2026-10-29), under the class-C grant of the
workspace instructions: "an llm wave for story:saved-codex-metadata-item-on-resume and its
release. Approved under the grant". Consumer: Harness 0.23.0, which resumes Codex sessions.

Conditions quoted from the item:

1. "I accept that story (record an approval-record citing D208, move it with the aep verbs)."
2. "Failing test first from a saved 0.16.0-shape transcript fixture, then the smallest fix (do not
   send a saved metadata item; do not drop any other item kind)."
3. "If other accepted, unblocked llm stories are independent and small, add up to 3 more (wave cap
   4); otherwise ship this alone." The store holds no other accepted story, so the wave has one unit.
4. "do NOT move the ESS pin" (D198).

## The option taken

Condition 2 decides it: none of the story's options a, b or c. The Responses egress
(`project_request`) leaves out an `Opaque` item of the selected binding whose payload is a
`response.metadata` stream event; every other opaque item is sent verbatim as before, and opaque
state of another binding is still refused before anything is left out.

Covered: one integration branch, one pull request through the bot, its landing on `main`, one
release and its verification. Not covered: registry publishing, an ESS pin move, every other story.
