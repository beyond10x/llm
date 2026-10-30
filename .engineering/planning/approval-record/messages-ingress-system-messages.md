---
format: aep.planning-md/3
id: approval-record:messages-ingress-system-messages
kind: approval-record
status: approved
title: 'D262: Messages ingress accepts a system entry inside messages, delivered alone'
tags:
- non-interactive
relations:
- decides: story:messages-ingress-system-messages
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-10-30T05:47:17Z", actor: "human:timo", revision: 2}
---
## The stop

Acceptance of `story:messages-ingress-system-messages` (`draft` to `active`), its implementation,
and its delivery alone as one pull request and one release.

## What authorises passing it

Session `b10x-chef-orchestrator`, decision D262 (2026-10-30), under the class-C grant of the
workspace instructions. Quoted from the item: "llm's Messages ingress must accept a role \"system\"
entry inside `messages`." The fix goes alone as one PR and one release under the delivery-rule
exception D169, because a consumer is blocked on it today.

The mapping, quoted from the item:

1. "Leading `system` entries, before the first non-system turn, are appended to the system prompt
   in order."
2. "A later `system` entry stays at its place as a user-role text block marked as system content.
   The mark must be one fixed, documented form, chosen by you and named in the CHANGELOG."
3. "Two adjacent user blocks are merged as the codec already does for alternation."
4. "Every backend gets the same mapping. If a backend's native format carries mid-conversation
   system content, use the native form there instead, and document it."
5. "Unknown content shapes inside a system entry are still refused, with the refusal naming the
   field path."

The run is non-interactive: the approval is the item, and nobody is asked again.

## Choices taken inside the item

- The mark is `<system-message>` + line feed + the entry's text + line feed + `</system-message>`.
- Condition 4: the mapping is made at ingress, onto the neutral turn, which has no item for an
  instruction between two turns, so every backend gets the marked user text. Using a backend's
  native form (a Chat `system` message or a Responses `developer` message in the middle) needs a
  new `llm_core::Item` variant; `Item` is matched exhaustively by consumers, so that is a breaking
  change outside this item, and is not done here.

## Covered

One branch, one pull request through the bot, its landing on `main`, one release and its
verification. Not covered: registry publishing, every other story.
