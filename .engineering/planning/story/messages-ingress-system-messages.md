---
format: aep.planning-md/3
id: story:messages-ingress-system-messages
kind: story
status: implemented
title: Messages ingress reads a system entry inside messages
summary: leading entries join the instructions; a later one stays in place as marked user text
relations:
- decomposes: epic:client-request-fields
- serves: vision:portable-model-inference
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-30T05:47:17Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-30T05:47:17Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-30T05:57:14Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1}}}
---
## Context

A headless Claude Code client (2.1.300) sends a `{"role":"system","content":...}` entry inside the
Messages `messages` array. `llm_messages::decode_request` refuses every such entry as a role
outside the subset (`crates/llm-messages/src/codec.rs` `decode_messages`), so a gateway built on
this decoder answers each such turn with 400 `translation-unsupported`, whatever backend the route
names. A headless Claude Code consumer of llm-gateway is blocked on it.

## Design

The mapping was decided by the orchestrator (decision D262, recorded in
`approval-record:messages-ingress-system-messages`):

- Leading `system` entries, before the first entry of any other role, are appended to the
  instructions in order, each joined by a line feed, as the blocks of `system` already are.
- A later `system` entry stays at its place as one user text item, its text wrapped in the fixed
  mark `<system-message>` + line feed + text + line feed + `</system-message>`
  (`llm_messages::SYSTEM_MESSAGE_OPEN`, `llm_messages::SYSTEM_MESSAGE_CLOSE`). One that follows an
  assistant tool call waits until the tool results that open the next user message are placed,
  because a user message answering a call must open with its results.
- Adjacent user items are merged into one user message by the Messages and Bedrock encoders, as
  they already do for alternation; Chat sends each user text as its own user message.
- The mapping happens at ingress, onto the neutral turn, so every backend (Messages, Bedrock
  Converse and Anthropic `InvokeModel`, Responses, Chat) receives the same items. The neutral turn
  has no item for an instruction between two turns; adding one would change the exhaustive
  `llm_core::Item` every consumer matches on, so no backend's native mid-conversation form is used.
- A `system` entry's content is a string or a list of `text` blocks (joined by a line feed); a
  `cache_control` mark on such a block is the `prompt-cache` hint as on any text block. A block of
  another type is refused by the path `messages[].content[].type`, any other key by its path. An
  entry with empty text carries nothing and adds nothing.

## Acceptance

- `crates/llm-messages/tests/system_messages.rs`: a request with one `system` entry at the start
  and one in the middle decodes, the first into the instructions and the second into a marked
  user item at its place; the projection sends one merged user message; the projection reads back
  as the same turn.
- The same file: a `system` entry's block of another type, or with another key, is refused by its
  path; a request of `system` entries alone is refused as without initial user content.
- `crates/llm-bedrock/tests/system_messages.rs`: the decoded turn projects onto Converse and
  `InvokeModel` with the leading entry in `system` and the later one as marked user text.
- Scenarios `contracts/messages/scenarios/ingress-reads-system-messages-as-instructions-and-marked-user-text.yaml`
  and `ingress-refuses-a-system-message-block-that-is-not-text.yaml`;
  `ingress-refuses-a-role-outside-the-subset.yaml` now uses role `tool`.

## Review

One adversary pass (2026-10-30) on the unit before delivery:

- Blocker, fixed in the same unit: a later `system` entry between an assistant tool call and the
  user message answering it was projected before the `tool_result`, which the route refuses. Such
  an entry now waits until the tool results that open the next user message are placed. Held by
  `crates/llm-messages/tests/adversary_d262_system_messages.rs`,
  `crates/llm-bedrock/tests/adversary_d262_system_messages.rs` and
  `system_messages.rs::a_system_entry_after_a_tool_call_follows_the_results_that_answer_it`.
- Warning, fixed: the docs said every projection merges the marked text with an adjacent user
  turn; Chat sends it as its own user message, and the docs now say so.
- Note, kept: the new refusal in `system_message` reuses the text of the `lib.rs` field refusal,
  so the falsification record counts it through that claim; no mutation targets it alone.
