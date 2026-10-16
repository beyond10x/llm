---
format: aep.planning-md/3
id: review-result:routes-ingress-acceptance-round-2
kind: review-result
status: active
title: Acceptance critic, round 2 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
- reviews: release-plan:llm-0-6-0
revision: 1
---
needs-revision

story:routes-document — the capabilities scenario names no observation that can show capabilities, because "observed through the serving model's port building" yields only a port kind, and `llm.routes.Loading` has no capabilities field; "one without takes the default" also names no default value to compare against — `.engineering/planning/story/routes-document.md:122` (spec/domains/routes.yaml, `llm.routes.Loading` fields; spec/domains/models.yaml, `llm.models.Construction.port`)

**What I read:** 4 of 4 ids (story:routes-document, story:responses-ingress-encoder, story:messages-ingress-encoder, release-plan:llm-0-6-0), whole bodies via `aep plan artifact show`. I also read review-result:routes-ingress-acceptance-round-1 and the epic, ran `aep plan artifact lifecycle release-plan`, and read `spec/domains/routes.yaml`, `spec/domains/models.yaml` and `.github/workflows/*.yml`. I compared the release plan with `release-plan:llm-0-5-1`.

Round 1 against this revision:
- Fixed: the unit-test list now names a check for each rule (keychain without a store, `~/` expansion, feature-off per feature, the marker-based no-leak test).
- Fixed: `nobody/model` and the bare unknown name now answer `invalid-request`.
- The capabilities finding above is new. Round 1 did not raise it.

**What I could not establish:**
- Whether the adapter may add a capabilities field to `llm.routes.Loading`. The story allows refining `spec/domains/routes.yaml` "only if the adapter needs it". I treated that as the fix path, not as a gap.
- Whether `llm.responses.EncodeIngress` and `llm.messages.EncodeIngress` fit their spec domains. That is design and scope, out of my lane.
- The line "an anonymous provider prepares nothing and succeeds" reads as `prepared=false` with no error. It is checkable, so I raised no finding.

```findings
- file: .engineering/planning/story/routes-document.md
  line: 122
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the capabilities scenario says capabilities are 'observed through the serving model's port building', but port building answers only a port kind and llm.routes.Loading has no capabilities field, and 'one without takes the default' names no default value, so neither half can be checked"
```
