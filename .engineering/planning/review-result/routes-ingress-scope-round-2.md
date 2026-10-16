---
format: aep.planning-md/3
id: review-result:routes-ingress-scope-round-2
kind: review-result
status: active
title: Scope critic, round 2 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
- reviews: release-plan:llm-0-6-0
revision: 1
---
approve

I extracted 8 promises from the parent and traced all 8 to an item. Both round 1 findings are fixed.

- **Release finding:** `release-plan:llm-0-6-0` now delivers the three stories and the epic. Its Execution and Completion evidence sections match AGENTS.md "Release" (`AGENTS.md:135-149`).
- **Capabilities-mapping finding:** the epic's Outcome now says "A route may carry its own capabilities" (epic Outcome section), and `spec/domains/routes.yaml` models the mapping form. The route mapping in `story:routes-document` is therefore traceable to the parent.

Nothing in the set reaches beyond the epic's exclusions. No story adds a server, a gateway, CLI or SDK change, or subscription OAuth, and the routes document refuses `subscription-oauth` by name. No two items claim the same outcome, and no promise is narrowed.

**What I read:** 5 artifacts (the epic, three stories, the release plan) with `aep plan artifact show`, plus `aep plan artifact graph`, `review-result:routes-ingress-scope-round-1` and `review-result:routes-ingress-design-round-1`. I also read `spec/domains/routes.yaml`, AGENTS.md "Release" and `release-plan:llm-0-5-1`. The promises, with the item that claims each:

| # | Promise | Claimed by |
|---|---|---|
| 1 | One `llm.routes/1` document read alike by gateway, CLI and SDK | `story:routes-document` |
| 2 | Responses ingress encoder | `story:responses-ingress-encoder` |
| 3 | Messages ingress encoder | `story:messages-ingress-encoder` |
| 4 | A route may carry its own capabilities | `story:routes-document` |
| 5 | Released as llm 0.6.0, tagged and published | `release-plan:llm-0-6-0` |
| 6 | `task check` passes with the baseline raised and nothing skipped | each story's conformance acceptance, plus the release plan's Gate evidence |
| 7 | No HTTP server | no item adds one |
| 8 | Subscription OAuth refused by name | `story:routes-document` acceptance (`kind: subscription-oauth`) |

**What I could not establish:**
- Whether the operator's plan really lists the commits, pushes, PRs, tag and Release that `release-plan:llm-0-6-0` names as authorized. I only have the plan's own statement of it.
- Out of my lane: the `gateway` provider is `bearer` with a `file:` secret in the story example and `anonymous` in the spec example. The round 1 scope critic raised this and it still holds. It belongs to the design and acceptance critics and does not set my verdict.

```findings
[]
```
