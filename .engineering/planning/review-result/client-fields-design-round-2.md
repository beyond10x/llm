---
format: aep.planning-md/3
id: review-result:client-fields-design-round-2
kind: review-result
status: active
title: Design critic, round 2, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
revision: 1
---
approve

I read the revised set and found no cycle, no split abstraction, no unrecorded dependency and no horizontal slice. Round 1 was also `approve`, with no findings to repeat.

**What I read:**
- **Artifacts:** the five stories, the epic (revision 2), `review-result:client-fields-design-round-1`, and `docs/design.md` "Client request fields" (lines 61-163).
- **Commands:** `aep plan artifact show` on each id, `relations`, `graph`, `validate` and `waves`. `validate` printed `valid`.
- **Code check:** I grepped the tree for the exported function names the stories cite (`project_request`, `decode_request`, `encode_request`, `ingest_request`, `decode_ingress_request`). All exist.

**Edges:**
- I walked 17 declared edges touching the set: 5 `decomposes`, 6 `serves` and 6 `depends_on`.
- I also read the review-result `reviews` edges in the graph output, so I looked outside the set.
- The shape is a diamond with no cycle. `neutral-turn-controls` comes first. The three wire stories each `depends_on` it. `client-turns-cross-wire` `depends_on` all three.
- No serialising chain. Each edge is a real read, and the three wire stories can run side by side.

**Acceptance reads:** I traced 13 reads to a producer in the set, and an edge records all 13.
- Three reads run from the wire stories to `neutral-turn-controls`: `Controls`, `wire_effort`, and `advisory_warning` with `Hints::omitted_by`. `field_path` is read the same way by the Chat and Responses stories.
- Ten reads run from `client-turns-cross-wire` to the wire stories:
  - the two fixtures: `crates/llm-messages/tests/fixtures/claude-code-first-turn.json` and `crates/llm-responses/tests/fixtures/codex/first-turn.json`
  - `decode_request`, `encode_request` and `omitted_advisories` from Messages
  - `ingest_request`, `project_request` and `omitted_advisories` from Responses
  - `project_request` and `omitted_advisories` from Chat

**Round 1 loose ends:**
- **Fixture paths:** now named. The Messages acceptance gives its fixture path, the Responses Design gives its fixtures, and the cross-wire Design reads both by `include_str!`.
- **Epic Stories list:** now lists all five stories.

**Advisory lists:** the exact lists the cross-wire story asserts match the design's advisory table and each wire story's `omitted_advisories` rule. The order follows the design's hint order.

**What I could not establish (all outside my lane, none set the verdict):**
- **Acceptance:** `story:neutral-turn-controls` lists `field_path` and `advisory_warning` in its Design but names no acceptance line that tests either.
- **Acceptance:** the cross-wire bullets say `decode_request` "then" `project_request`. `llm_messages::decode_request` returns an `IngressRequest`, not a `TurnRequest` (`crates/llm-messages/src/codec.rs:192`), so the step between the two is unstated.
- **Parallel safety:** `waves` still places `story:responses-ingress-client-fields` in wave 6 and the other two wire stories in wave 3. I did not trace the cause.
- **Parallel safety:** `story:neutral-turn-controls` and `story:responses-ingress-client-fields` both edit `crates/llm-responses/src/request.rs`. The `depends_on` edge orders them, so this is not a shape defect.

```findings
[]
```
