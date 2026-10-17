---
format: aep.planning-md/3
id: review-result:client-fields-scope-round-2
kind: review-result
status: active
title: Scope critic, round 2, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
- reviews: epic:client-request-fields
revision: 1
---
needs-revision

story:responses-ingress-client-fields — adds an `encode_request` refusal for a conversation whose identifier differs from the turn's `prompt_cache_key`. Neither the epic nor `docs/design.md` "Client request fields" states it, which is the same defect as round-1's 512-byte cap. The design should state the rule first, or the body should drop it from the Design and Acceptance lines. — `.engineering/planning/story/responses-ingress-client-fields.md:57` (also `:92`; the section has no "conversation" outside the Responses carry rule, `docs/design.md:61-153`)

Round-1 findings:
- **`safeguards` and `system` message inside `messages`:** fixed, and the finding does not repeat. `story:messages-ingress-client-fields` now lists `safeguards` and the `system` message in its refused-by-path acceptance (`.engineering/planning/story/messages-ingress-client-fields.md:78-84`). The design's "Still refused" paragraph names the mid-conversation message.
- **512-byte cap:** fixed. `docs/design.md` now states "A cache key is non-empty and at most 512 bytes" (line 83 of the design section).
- **Epic story list:** fixed. It now names `story:client-turns-cross-wire`.

**What you read:** 7 artifacts (the epic, the 5 stories, round-1's `review-result`) and `docs/design.md` "Client request fields". Commands run:
- `aep plan artifact show` on each artifact
- `aep plan artifact graph`
- `aep plan artifact list --kind story --status draft`
- `grep` and `sed` on the store files, `docs/design.md` and `docs/responses.md`

I extracted 15 promises from the epic and traced all 15 to an item, with no double claims. The effort table, `omitted_advisories` and the other effort and advisory claims sit at different layers, so I did not count them as double claims. Nothing else in the graph claims part of the epic.

**What you could not establish:**
- Whether the conversation-identifier refusal is an intended consequence of carrying `prompt_cache_key` on Responses egress. `docs/responses.md:417-421` says ingress did not read the key before, so the rule may be needed. The design has to say so, and that is the fix.
- Out of my lane, and not setting the verdict: `story:neutral-turn-controls` lists `crates/llm-tool-call/src/lib.rs` in its scope as `inferred` and the body never mentions it (design critic).

```findings
- file: .engineering/planning/story/responses-ingress-client-fields.md
  line: 57
  category: scope
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the item adds an encode_request refusal for a conversation whose identifier differs from the turn's prompt_cache_key, a rule that appears in no sentence of the epic or of docs/design.md 'Client request fields'; have the design state it first, or drop it from the Design and Acceptance"
```
