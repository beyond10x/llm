---
format: aep.planning-md/3
id: review-result:client-fields-acceptance-round-2
kind: review-result
status: active
title: Acceptance critic, round 2, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
revision: 1
---
needs-revision
story:chat-ingress-closed-levels — the acceptance bullet "`omitted_advisories` and the client's single trailing `advisory-omitted` warning." is a noun phrase that states no outcome (no returned list, no success-turn emission, no absence on a failed or nothing-omitted turn), so it reads the same before and after the work, unlike the Messages bullet that says "absent when nothing is omitted and absent from a failed turn" — .engineering/planning/story/chat-ingress-closed-levels.md:79
story:responses-ingress-client-fields — no acceptance bullet states what `omitted_advisories` returns or that `ResponsesClient` emits one `advisory-omitted` warning after a successful turn and none after a failed one, though the Design promises both; the only trace is "are reported omitted" with no stated mechanism — .engineering/planning/story/responses-ingress-client-fields.md:89 (Design at :59-60)
story:neutral-turn-controls — the acceptance names no check for `llm_core::field_path` (a bounded key copied, an over-long or foreign-shaped key not) or for `advisory_warning` (code `advisory-omitted`, the names listed), so both could be absent and the story still close — .engineering/planning/story/neutral-turn-controls.md:76 (Design at :58-61)
story:client-turns-cross-wire — the bullets assert `omitted_advisories` is "exactly `[prompt-cache-key, reasoning-summary, input-item-ids, client-metadata]`" as an ordered list, but no story pins the `Advisory` order, so two correct implementations give different answers; the bullet should say "as a set" or name the order (the design's Advisory hints table) — .engineering/planning/story/client-turns-cross-wire.md:53

Round 1 status: all five round-1 findings are fixed.
- `neutral-turn-controls` now lists the `wire_effort` rows it covers and states that the spec names the new types.
- `messages-ingress-client-fields` and `responses-ingress-client-fields` enumerate the round-trip rules and assert the one-way rules as one-way.
- `client-turns-cross-wire` now names per-field body values.

The four findings above are new. The first three are acceptance gaps the round-1 revision left or exposed, and the fourth comes from the revised cross-wire bullets.

**What I read:** all 5 story ids, whole bodies, with `aep plan artifact show`. I also read `review-result:client-fields-acceptance-round-1`, `aep plan artifact lifecycle story`, the epic body, `docs/design.md:61-154`, the Claude Code fixture (it has no `metadata`, so `[prompt-cache]` is consistent), `docs/messages.md` on "marked", and the `0.6.0` tag (present).

**What I could not establish:**
- Whether the Codex capture carries `reasoning.effort` `high` and four function tools, as the cross-wire bullets assume. The fixture does not exist yet and the epic's field list does not name effort.
- Out of my lane, so it did not set the verdict: `story:responses-ingress-client-fields` has no bullet for the `reasoning.summary` or `text.verbosity` refusals, which belongs to coverage.

```findings
- file: .engineering/planning/story/chat-ingress-closed-levels.md
  line: 79
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance bullet 'omitted_advisories and the client's single trailing advisory-omitted warning.' states no outcome: no returned list, no emission after a successful turn, no absence on a failed or nothing-omitted turn"
- file: .engineering/planning/story/responses-ingress-client-fields.md
  line: 89
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "no acceptance bullet states what omitted_advisories returns or that ResponsesClient emits one advisory-omitted warning after a successful turn and none after a failed one, though the Design promises both; the only trace is 'are reported omitted'"
- file: .engineering/planning/story/neutral-turn-controls.md
  line: 76
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the acceptance names no check for llm_core::field_path (a bounded key copied, an over-long or foreign-shaped key not) or for advisory_warning (code advisory-omitted, names listed), so both could be absent and the story still close"
- file: .engineering/planning/story/client-turns-cross-wire.md
  line: 53
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the bullets assert omitted_advisories is 'exactly' an ordered list, but no story pins the Advisory order, so two correct implementations give different answers; the bullet should say 'as a set' or name the order"
```
