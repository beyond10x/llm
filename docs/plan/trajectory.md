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
- Read again on 2026-11-25 at `main` `831c21d95`: the same five pins and no open blocker on llm. No
  consumer names a gap, so the plan after wave 8 is ordered by the kind of story.

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
| 8 | `story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason` | O6 | a Bedrock Converse turn that stops for an unmodelled reason without content is an error a caller can classify, with its usage | delivered: [wave-2026-11-25-converse-empty-turn](wave-2026-11-25-converse-empty-turn.md) |

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

## Wave plan after wave 8

The seven stories accepted on 2026-11-25
(`approval-record:backlog-2026-11-25-seven-drafts-accepted`), read at `main` `831c21d95` after each
got its Scope section from one `story-scoper`. `aep plan artifact waves --kind story --status active`
(aep 0.80.0) over the nine active stories: 6 waves, 133 collisions, 0 unassessed. Of the seven, one
pair does not collide: the `llm-decide` story and the cancellation story. They share wave 9; each
later wave holds one story.

| Wave | Story | Objective (the coordinator's reading) | Makes true | State |
| --- | --- | --- | --- | --- |
| 9 | `story:llm-decide-tests-read-no-clock-and-build-one-binary`, `story:messages-cancellation-case-waits-on-a-signal` | none: the organisation's test rule | no test of `llm-decide` reads the clock and the crate links one integration-test binary; the cancellation case of `llm-messages` waits on a signal and holds no `sleep` | open: [wave-2026-11-25-tests-without-a-clock](wave-2026-11-25-tests-without-a-clock.md) |
| 10 | `story:test-binaries-one-per-crate` | none: the organisation's test rule | every crate with integration tests links one test binary, with the same number of tests | planned |
| 11 | `story:replay-stops-at-the-last-case-have-scenarios` | O6 | two scenarios hold the stop of a replay at its last case; the conformance floor is 1092 | planned |
| 12 | `story:tool-arguments-with-a-repeated-key` | O6 | tool-call arguments that give a key twice are a protocol error on every wire, and the classifier abstains on them | planned |
| 13 | `story:responses-carries-refusal-text` | O3 | a Responses answer, request and ingress carry refusal text as its own item, and selection admits a Responses target for it | planned |
| 14 | `story:cli-route-explain` | O1 | `llm route explain <route>` prints each target with its rejection names and the chosen target, without a network call | planned |

The verb's order was the `llm-decide` story, then `story:cli-route-explain` with the cancellation
story, the replay story, the refusal story, the workspace story, the repeated-key story. The order
above is the acceptance's: test defects first, correctness second, features last. Inside each
kind it is the coordinator's:

- The two one-crate test stories run before the workspace story, which moves the files they edit
  (`crates/llm-decide/tests`, `crates/llm-messages/tests/client.rs`).
- The replay story adds data only and runs before the repeated-key story, which changes nine parse
  sites and is breaking.
- The refusal story runs before `story:cli-route-explain`, so the command's guide is written
  against the final condition of the `refusal-text` rejection.
- No `depends_on` is crossed: `story:cli-route-explain` depends on
  `story:output-and-image-capabilities`, which is `implemented`.

Wave 7 keeps its number and is not in this order: its pull request stays open until a `ux-browser`
release lets the docs UX gate end within its 120 s budget. `story:anthropic-access` stays out of the
plan under its credential blocker.

What the scopers found, each in its story's Scope section, and the reading the coordinator takes
into the wave's proposal:

- Wave 9, `llm-decide`: two cases compare durations (`tests/judge.rs`,
  `tests/adversary_w27_judge_pass2.rs`). A counter under `#[cfg(test)]` cannot be read from
  `tests/`, so the two cases become unit tests beside the reader in `src/answer.rs` and count the
  reader's work. The same unit makes the crate's five test binaries one, and wave 10 leaves the
  crate alone.
- Wave 9, cancellation: a signal the server gives after it wrote the head does not prove that the
  client is blocked in its sink, so the case would still race. The case waits on the server's
  signal and on the sink's `emit` being entered, as `AcceptingSink` in `tests/live_connection.rs`
  does. `the_turn_deadline_bounds_a_sink_that_never_accepts` in the same file has the same shape and
  goes into the list the story's third acceptance line asks for.
- Wave 10: 247 `tests/*.rs` files in 20 crates, 17 crates with more than one. Three targets with
  `harness = false` (`llm-bedrock` `process_credentials`, `llm-models` `process_routes`, `llm-cli`
  `credential_process`) are programs and cannot become modules: the first acceptance line needs a
  word on them, asked with the wave's proposal. Nine tests start their own binary with
  `--exact <bare name>`; three of them would pass without running a test after the move, so each
  gets its module path and an assertion that the child ran one test. The unit is split by crate,
  two parts at a time, the documents last. `story:llm-calls-builds-one-integration-test-binary`
  stays a draft: this story covers its crate.
- Wave 11: two scenario files, each asserting `"not_sent":0`; the floor moves from 1090 to 1092.
- Wave 12: decided on 2026-11-25 (the story's section Open): a repeated key is a protocol error,
  like arguments that are not JSON, and the CHANGELOG line is marked breaking. Nine parse sites
  on five wires; one strict reader in `llm-core` replaces the two copies in `llm-decide` and
  `llm-openai-decisions`.
- Wave 13: a `refusal` content part fails a Responses turn as `unsupported` today, and only Chat
  Completions carries refusal text; Messages and Bedrock Converse keep refusing it. The wire names
  (`response.refusal.delta`, the `refusal` part) are in no recorded fixture of this repository, and
  the wave makes no live call. An answer that was an error becomes a returned turn: the CHANGELOG
  line names it.
- Wave 14: a route of `llm.toml` expands to one target, so the command prints one candidate there;
  a Bedrock route and a direct `<endpoint>/<model-id>` name have no catalog entry and are refused by
  name; without an input-token bound every target is rejected `input-tokens-unknown`, so the
  command takes an optional `--input-tokens` and prints that rejection without it. No domain
  describes the `llm` command, so the wave adds no scenario and the floor does not move.

## Releases

- 0.27.0 holds wave 1, 0.28.0 wave 2 and 0.29.0 wave 3. Each later wave is one minor release.
- Wave 4 changes what a caller of `llm-messages` and of the Messages ingress gets for an answer
  that was a success before (two lines marked breaking in the CHANGELOG); its release is 0.30.0.
- Wave 5 makes a model classifier abstain on an answer it took before (one line marked breaking);
  its release is 0.31.0.
- Wave 6 changes the gate and no library: the ESS pin is 0.99.1 and the local gate runs the pinned
  `ess`; its release is 0.32.0.
- Wave 8 makes a Bedrock Converse turn that stops for an unmodelled reason without content an error
  where it was a success before (one line marked breaking in the CHANGELOG); its release is 0.33.0.
- Each wave after wave 8 that has a CHANGELOG entry is one minor release, numbered in the order the
  waves merge; waves 9 and 10 change tests only and ship with the next such release. Wave 7 takes
  the next number when its gate is green. Wave 12 carries one line marked breaking; wave 13 names
  the answer that was an error and becomes a returned turn.
- Still drafts: `story:the-neutral-turn-in-ess` and
  `story:llm-calls-builds-one-integration-test-binary`.

## Backlog filed by this plan

Accepted on 2026-11-24 (`approval-record:backlog-2026-11-24-four-drafts-accepted`); each is `active`
and is in the wave plan of 2026-11-25 above:

- `story:messages-empty-turn-with-an-unmodelled-stop-reason` (issue 83).
- `story:ess-pin-current-and-local-gate-finds-it`: host `ess` 0.98.0 against the pinned 0.87.0.
- `story:classifier-takes-only-a-whole-answer`: the class the judge was corrected for, in the classifier since 0.23.0.
- `story:docs-ux-layout-checks-from-ux-browser`: proposed by the uxui repository on 2026-11-24; it
  starts only when a release tag of that repository that carries `ux-browser` exists.

Accepted on 2026-11-25 (`approval-record:backlog-2026-11-25-seven-drafts-accepted`): the five
stories below that name a wave, with `story:test-binaries-one-per-crate` (wave 10),
`story:responses-carries-refusal-text` (wave 13) and `story:cli-route-explain` (wave 14), which were
filed before this plan.

Filed by wave 2, now wave 9:

- `story:llm-decide-tests-read-no-clock-and-build-one-binary`: a wall-time case of wave 1 failed once
  under load; the crate builds five integration-test binaries.

Filed by wave 3, the first now wave 11, the second still a draft that wave 10 covers:

- `story:replay-stops-at-the-last-case-have-scenarios`: the stop of a replay at its last case is held
  by Rust tests only.
- `story:llm-calls-builds-one-integration-test-binary`: the crate builds seven; the wave added the
  seventh.

Filed by wave 4, now wave 9 (its other draft was wave 8):

- `story:messages-cancellation-case-waits-on-a-signal`: one case of `llm-messages` cancels after a
  fixed 100 ms and failed once of 42 runs under load.

Filed at the opening of wave 5, now wave 12:

- `story:tool-arguments-with-a-repeated-key`: tool-call arguments that give a key twice reach a
  caller with the last value chosen.
