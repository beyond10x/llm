---
format: aep.planning-md/3
id: review-result:routes-ingress-scope-round-1
kind: review-result
status: active
title: Scope critic, round 1 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
revision: 1
---
needs-revision

- epic:routes-and-ingress — the promise "release 0.6.0 is tagged and published as AGENTS.md "Release" describes" is claimed by no item in the set, and only the epic file mentions 0.6.0 anywhere in the store. A `release-plan:llm-0-6-0` delivering the three stories would take it, as `release-plan:llm-0-5-1` did for 0.5.1. The alternative is a line in the epic saying the release is deliberately left to a later release-plan. — .engineering/planning/epic/routes-and-ingress.md:26 (also :18, "Released as llm 0.6.0."); `grep -rln -E "0\.6\.0|0-6-0" .engineering/planning` returns only that file
- story:routes-document — the design lets a route be written as a mapping `{model: ..., capabilities: {...}}`. Neither the epic nor `spec/domains/routes.yaml` asks for it, and the spec models a route only as a `<provider>/<model-id>` string. The body should drop the mapping form and keep the fixed default, or the spec and epic should gain the promise. — .engineering/planning/story/routes-document.md:59; spec/domains/routes.yaml:16-18,20-22

What I read: 4 artifacts, using `aep plan artifact show` on the epic and the three stories, plus `aep plan artifact graph` and `aep plan artifact kinds`. I also read `spec/domains/routes.yaml`, the AGENTS.md "Release" and "Planning store and waves" sections, `release-plan:llm-0-5-1`, and the epic file for line numbers.

Promises extracted from the epic: 6. I traced 5 to an item:
1. `llm.routes/1` read alike by the gateway, CLI and SDK — routes-document.
2. Responses ingress encoder — responses-ingress-encoder.
3. Messages ingress encoder — messages-ingress-encoder.
4. `task check` passes with the baseline raised and nothing skipped — each story's conformance acceptance raises the baseline.
5. Subscription OAuth is refused by name — routes-document acceptance.
6. Release 0.6.0 — not claimed.

Two items claiming one outcome: none found. A promise silently narrowed: none found.

What I could not establish:
- I did not see the drafter's report. If it names the release as deliberately deferred, finding 1 falls.
- The wording of "`task check` passes on the merged wave" says nothing of which story carries it. I treated it as covered by the stories' conformance acceptances.
- Out of my lane: the routes-document example gives the `gateway` provider `bearer` with a `file:` secret, while the spec example gives it `anonymous` (routes-document.md:30-31 against routes.yaml:14-15). That is for the design and acceptance critics, and it does not set my verdict.

```findings
- file: .engineering/planning/epic/routes-and-ingress.md
  line: 26
  category: scope
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the promise 'release 0.6.0 is tagged and published as AGENTS.md \"Release\" describes' is claimed by no item in the set (a release-plan delivering the three stories would take it, as release-plan:llm-0-5-1 did for 0.5.1), and the epic does not say it is deferred"
- file: .engineering/planning/story/routes-document.md
  line: 59
  category: scope
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the design adds a route-as-mapping form with a per-route capabilities override that neither the epic nor spec/domains/routes.yaml asks for, which models a route only as a <provider>/<model-id> string"
```
