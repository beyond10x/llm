---
format: aep.planning-md/3
id: review-result:client-fields-design-round-1
kind: review-result
status: active
title: Design critic, round 1, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
revision: 1
---
approve

I read all five stories, the epic and the "Client request fields" section of `docs/design.md`. I found no cycle, no split abstraction and no unrecorded dependency.

- **Edges:** I walked the 17 declared edges that touch the set. None leave the set except `decomposes` to `epic:client-request-fields` and `serves` to `vision:portable-model-inference`.
- **Shape:** `neutral-turn-controls` is first, the three wire stories each `depends_on` it, and `client-turns-cross-wire` `depends_on` all three. That is a diamond, not a queue, with no cycle, and each wire story is demonstrable on its own.
- **Acceptance reads:** I traced 9 reads to a producer in the set, and an edge records all 9. Each wire story reads `Controls`, `wire_effort`, `advisory_warning` and `Hints::omitted_by` from the neutral story. The cross-wire story reads the Claude Code fixture, the Codex capture, `project_request`, `encode_request`, `ingest_request`, `decode_request` and `omitted_advisories` from the three wire stories.
- **Advisory lists:** the three `omitted_advisories` lists match the design's advisory-hints table and the lists the cross-wire story asserts.
- **Neutral story and the wire stories:** the neutral story owns the effort table and the controls types, and the wire stories own the projections. The enabled-thinking-budget refusal appears in `story:neutral-turn-controls` (the `wire_effort` table rows) and in the Chat acceptance. They check different layers, so this is not a split.
- **Cross-wire story:** it is a test-only story, but the per-wire stories each carry their own tests, so it is not a horizontal slice. Its Context gives the reason: only `checks/conformance` depends on all three protocol crates.

What I read: 5 stories, the epic, `story:messages-ingress-encoder`, `story:responses-ingress-encoder`, `story:unattributed-opaque-state` and `story:routes-document`, plus `docs/design.md` lines 55-192 and `checks/conformance/Cargo.toml`. Commands: `aep plan artifact show`, `relations`, `graph`, `validate` (valid) and `waves`.

What I could not establish, all outside my lane, none of which set the verdict:
- **Fixture paths:** neither `story:messages-ingress-client-fields` nor `story:responses-ingress-client-fields` names where its fixture lives. The cross-wire story must read both, so this is a point for the acceptance and parallel-safety critics.
- **Epic body:** the epic's "Stories" list names four stories and omits `story:client-turns-cross-wire`. This is a scope question.
- **Wave placement:** `waves` puts `story:responses-ingress-client-fields` in wave 6, after the other two wire stories in wave 3. No edge in the set explains this, so it is likely scope-overlap ordering, which is a parallel-safety matter.

```findings
[]
```
