# Trajectory and wave plan, 2026-11-24

Read at `main` `c7c2a62cb` (llm 0.26.0). The wave plan below was derived with
`aep plan artifact waves --kind story --status active` (aep 0.80.0): 4 waves of one story,
14 collisions, 0 unassessed. Each wave page links back here.

## Vision

- One neutral model interface, embedded in an application or served by a gateway
  (`vision:portable-model-inference`).
- Protocol, provider and account, credential source, model, routing and cost stay separate and explicit.
- It serves the organisation objectives O1 (explicit reach, credentials and spend), O3 (one interface
  for several harnesses) and O6 (attributable usage, cost and failures).

## Consumers

- No live consumer holds an open blocker on llm: llm-gateway's three `dependency-blocker`s on llm are
  `cleared`; inference, loom, harness and rfcspec carry none.
- Every live consumer pins behind 0.26.0: llm-gateway 0.24.0, inference 0.20.0, rfcspec 0.20.0,
  harness 0.17.0, loom 0.8.1 (each repository's `main`, read 2026-11-24).
- The first gap is the adopter report of 2026-11-05 (tag `from-adopter-report`): 10 of its 14 stories
  are `implemented`, 3 are `active` (the judge, the decision endpoint, the replay), 1 is a `draft`.
- Issue 83 (a Messages turn that stops for an unmodelled reason without content decodes as a success)
  has no accepted story; it is filed as a draft and waits behind this plan.

## Wave plan

| Wave | Story | Objective (the coordinator's reading) | Makes true | State |
| --- | --- | --- | --- | --- |
| 1 | `story:rubric-judge` | O6 | `llm-decide` judges a subject against a rubric and returns a verdict that names the rubric version, its digest, the route target and the upstream model; 6 scenarios | delivered: [wave-2026-11-24-rubric-judge](wave-2026-11-24-rubric-judge.md) |
| 2 | `story:openai-decisions-client` | O3 | the classifier's third model path asks a provider decision endpoint (`POST /v1/decisions`) through llm's credentials, transport and call records; 6 scenarios | planned |
| 3 | `story:offline-replay-eval` | O6 | `llm replay` sends a recorded corpus to a candidate route, compares each answer and stops at a money cap; 6 scenarios | planned |

The verb's order was `story:anthropic-access`, `story:offline-replay-eval`,
`story:openai-decisions-client`, `story:rubric-judge`. The replay compares turns, classifications and
verdicts, so it runs after the two stories that add a classification path and the verdict; no
collision and no `depends_on` is crossed by that order. `story:anthropic-access` left the plan: it is
held by `credential-blocker:anthropic-live-qualification`.

## Releases

- 0.27.0 holds wave 1. Each later wave is one minor release.
- After the plan: the drafts the owner accepts, first the story for issue 83, then
  `story:the-neutral-turn-in-ess`, `story:responses-carries-refusal-text` and `story:cli-route-explain`.

## Backlog filed by this plan

- `story:messages-empty-turn-with-an-unmodelled-stop-reason` (issue 83), `draft`.
- `story:ess-pin-current-and-local-gate-finds-it`, `draft`: host `ess` 0.98.0 against the pinned 0.87.0.
- `story:classifier-takes-only-a-whole-answer`, `draft`: the class the judge was corrected for, in the classifier since 0.23.0.
- `story:docs-ux-layout-checks-from-ux-browser`, `draft`: proposed by the uxui repository on 2026-11-24.
