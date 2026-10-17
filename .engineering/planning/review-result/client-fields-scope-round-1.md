---
format: aep.planning-md/3
id: review-result:client-fields-scope-round-1
kind: review-result
status: active
title: Scope critic, round 1, epic:client-request-fields
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

story:messages-ingress-client-fields — the epic promises the Claude Code 2.1.301 fields `safeguards` and a `system` message inside `messages` are "refused by name and listed as follow-up", and no item or design list names either (the design's "Still refused" paragraph names only `context_management`, `thinking.display` and the mid-conversation `system` message). The body should name `safeguards` and a `system`-role message in its refused-by-path list, or the follow-up list should be written — `.engineering/planning/epic/client-request-fields.md:44-47`
story:neutral-turn-controls — the 512-byte cap on `prompt_cache_key` has no sentence in the epic or in `docs/design.md` "Client request fields" (neither contains "512"), so the item adds a refusal rule the parent did not ask for; drop it, or have the design state it first — `.engineering/planning/story/neutral-turn-controls.md:65`

**What you read:** 6 artifacts (the epic, the 5 stories) and the design section. I ran `aep plan artifact show` on each, `aep plan artifact graph`, `aep plan artifact list --kind story --status draft`, and `grep` and `nl` on the store files. The epic yielded 14 promises, and I traced 13 to an item. The 14th is the `safeguards` and messages-`system` follow-up, which is the first finding.

**What you could not establish:**
- Whether a follow-up list for `safeguards` exists in the drafter's report. I did not see it, and nothing in the store holds it.
- The epic's "Stories" section names four stories and not `story:client-turns-cross-wire`. That story traces to the epic's outcome ("crosses wires"), so I did not count it as reach. The epic's story list should name it. That is a revision to the epic, not to an item.
- Out of my lane:
  - The acceptance for both Codex-turn stories never exercises `text.verbosity`, although the epic lists it as captured. It is not asserted in the Responses story's first-turn acceptance or in the Codex-to-Messages and Codex-to-Chat cross-wire cases (acceptance critic).
  - The effort table is exercised in three places: `neutral-turn-controls`, the Chat story and the cross-wire story. They test different layers, so I did not call it a double claim (design critic).

```findings
- file: .engineering/planning/epic/client-request-fields.md
  line: 44
  category: scope
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the epic promises the Claude Code 2.1.301 fields `safeguards` and a `system` message inside `messages` are refused by name and listed as follow-up, and no item or design list names either; story:messages-ingress-client-fields should name them in its refused-by-path list, or the follow-up list should be written"
- file: .engineering/planning/story/neutral-turn-controls.md
  line: 65
  category: scope
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the 512-byte cap on prompt_cache_key appears in no sentence of the epic or of docs/design.md 'Client request fields', so the item adds a refusal rule the parent did not ask for; drop it or have the design state it first"
```
