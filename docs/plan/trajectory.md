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
- The first gap is the adopter report of 2026-11-05 (tag `from-adopter-report`): 13 of its 14 stories
  are `implemented` (the judge, the decision endpoint and the replay since waves 1 to 3), 1 is a
  `draft`.
- Issue 83 (a Messages turn that stops for an unmodelled reason without content decodes as a success)
  is the one gap a caller of the library reported; its story is wave 4.

## Wave plan

| Wave | Story | Objective (the coordinator's reading) | Makes true | State |
| --- | --- | --- | --- | --- |
| 1 | `story:rubric-judge` | O6 | `llm-decide` judges a subject against a rubric and returns a verdict that names the rubric version, its digest, the route target and the upstream model; 6 scenarios | delivered: [wave-2026-11-24-rubric-judge](wave-2026-11-24-rubric-judge.md) |
| 2 | `story:openai-decisions-client` | O3 | the classifier's third model path asks a provider decision endpoint (`POST /v1/decisions`) through llm's credentials, transport and call records; 6 scenarios | delivered: [wave-2026-11-24-openai-decisions-client](wave-2026-11-24-openai-decisions-client.md) |
| 3 | `story:offline-replay-eval` | O6 | `llm replay` sends a recorded corpus to a candidate route, compares each answer and stops at a money cap; 6 scenarios | delivered: [wave-2026-11-24-offline-replay-eval](wave-2026-11-24-offline-replay-eval.md) |

The verb's order was `story:anthropic-access`, `story:offline-replay-eval`,
`story:openai-decisions-client`, `story:rubric-judge`. The replay compares turns, classifications and
verdicts, so it runs after the two stories that add a classification path and the verdict; no
collision and no `depends_on` is crossed by that order. `story:anthropic-access` left the plan: it is
held by `credential-blocker:anthropic-live-qualification`.

## Wave plan of 2026-11-25

The four stories accepted on 2026-11-24, read at `main` `c8e57d225` after each got its Scope section
from one `story-scoper`. `aep plan artifact waves --kind story --status active` (aep 0.80.0): 4 waves,
38 collisions, 0 unassessed. Every pair of the four collides on `AGENTS.md` and `CHANGELOG.md`, so
each wave holds one story.

| Wave | Story | Objective (the coordinator's reading) | Makes true | State |
| --- | --- | --- | --- | --- |
| 4 | `story:messages-empty-turn-with-an-unmodelled-stop-reason` | O6 | a Messages turn that stops for an unmodelled reason without content is an error a caller can classify, with its usage; issue 83 closes with the release | delivered: [wave-2026-11-25-messages-empty-turn](wave-2026-11-25-messages-empty-turn.md) |
| 5 | `story:classifier-takes-only-a-whole-answer` | O6 | the JSON-schema classifier decides only on a whole answer, read by the judge's reader | delivered: [wave-2026-11-25-classifier-whole-answer](wave-2026-11-25-classifier-whole-answer.md) |
| 6 | `story:ess-pin-current-and-local-gate-finds-it` | O3 | the ESS pin is the current release and the local conformance step runs the pinned `ess` or refuses another | delivered: [wave-2026-11-25-ess-pin](wave-2026-11-25-ess-pin.md) |
| 7 | `story:docs-ux-layout-checks-from-ux-browser` | O3 | the docs UX gate takes its five layout checks from a pinned `ux-browser` release | planned; the uxui repository published release 0.8.1 with `ux-browser` on 2026-11-25 |
| 8 | `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason` | O6 | a Bedrock Converse turn that stops for an unmodelled reason without content is an error a caller can classify, with its usage | planned; accepted on 2026-11-25 (`approval-record:backlog-2026-11-25-bedrock-converse-accepted`) |

The verb's order was the docs UX story, the classifier, the ESS pin, the Messages story. The order
above is the coordinator's: issue 83 first, because a caller reported it; the ESS pin after the two
stories that add scenarios, so the suite and the 314 schema files are regenerated once with the new
`ess`; the docs UX story last, because it waits on another repository's release. No `depends_on` is
crossed. `story:anthropic-access` stays out of the plan under its credential blocker.

Settled at the opening of wave 5: a repeated key in a forced tool call's arguments is not visible to
`llm-decide` (the three protocol decoders keep the last value) and is the draft
`story:tool-arguments-with-a-repeated-key`. Open before wave 7: the patch offered by the
uxui repository leaves `ux/baseline.json` and `ux/core/layout.ts` as they are and commits no
planted-defect test; the story asks for all three.

## Releases

- 0.27.0 holds wave 1, 0.28.0 wave 2 and 0.29.0 wave 3. Each later wave is one minor release.
- Wave 4 changes what a caller of `llm-messages` and of the Messages ingress gets for an answer
  that was a success before (two lines marked breaking in the CHANGELOG); its release is 0.30.0.
- Wave 5 makes a model classifier abstain on an answer it took before (one line marked breaking);
  its release is 0.31.0.
- Wave 6 changes the gate and no library: the ESS pin is 0.99.1 and the local gate runs the pinned
  `ess`; its release is 0.32.0.
- Still drafts: `story:the-neutral-turn-in-ess`, `story:responses-carries-refusal-text` and
  `story:cli-route-explain`.

## Backlog filed by this plan

Accepted on 2026-11-24 (`approval-record:backlog-2026-11-24-four-drafts-accepted`); each is `active`
and is in the wave plan of 2026-11-25 above:

- `story:messages-empty-turn-with-an-unmodelled-stop-reason` (issue 83).
- `story:ess-pin-current-and-local-gate-finds-it`: host `ess` 0.98.0 against the pinned 0.87.0.
- `story:classifier-takes-only-a-whole-answer`: the class the judge was corrected for, in the classifier since 0.23.0.
- `story:docs-ux-layout-checks-from-ux-browser`: proposed by the uxui repository on 2026-11-24; it
  starts only when a release tag of that repository that carries `ux-browser` exists.

Filed by wave 2 and still a draft (accepting it is the owner's act):

- `story:llm-decide-tests-read-no-clock-and-build-one-binary`: a wall-time case of wave 1 failed once
  under load; the crate builds five integration-test binaries.

Filed by wave 3 and still drafts:

- `story:replay-stops-at-the-last-case-have-scenarios`: the stop of a replay at its last case is held
  by Rust tests only.
- `story:llm-calls-builds-one-integration-test-binary`: the crate builds seven; the wave added the
  seventh.

Filed by wave 4 and still drafts:

- `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason`: Bedrock Converse has its own
  copy of the reason mapping and still decodes such a turn as a success.
- `story:messages-cancellation-case-waits-on-a-signal`: one case of `llm-messages` cancels after a
  fixed 100 ms and failed once of 42 runs under load.

Filed at the opening of wave 5 and still a draft:

- `story:tool-arguments-with-a-repeated-key`: tool-call arguments that give a key twice reach a
  caller with the last value chosen.
