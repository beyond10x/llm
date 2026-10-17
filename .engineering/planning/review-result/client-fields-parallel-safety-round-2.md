---
format: aep.planning-md/3
id: review-result:client-fields-parallel-safety-round-2
kind: review-result
status: active
title: Parallel-safety critic, round 2, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
revision: 1
---
approve

I read all five stories and the round-1 result, and ran the waves and graph commands. Round 1's three findings (the wire stories had their branches edit `contracts/ess-inputs.yaml`) are fixed. Each wire story's Shared files section now says the coordinator lists the scenarios in that file and regenerates `contracts/suite.json`, `contracts/schema/` and `contracts/baseline.json`. The branch regenerates them only in an uncommitted working copy to run its own check. This matches the plan you gave.

**What I read:** 5 stories plus `review-result:client-fields-parallel-safety-round-1`.
- Commands: `aep plan artifact show` on each story and on the round-1 result, `aep plan artifact graph`, and `aep plan artifact waves --kind story --status draft --format json`.
- Tree reads: `checks/conformance/Cargo.toml`, a listing of `checks/conformance/{src,tests}`, `git grep "TurnRequest {"`, and `git grep` for `ess-inputs.yaml` and `suite.json` in `checks/conformance/src`.
- Surfaces:
  - Fully cited: 3 stories (messages, responses and chat ingress).
  - Cited in the main paths and inferred in the rest: 2 stories (`story:neutral-turn-controls`, `story:client-turns-cross-wire`).
  - Unplaceable: 0.
- Acceptance reads traced to a producer in the set: 6 of 6, and the `depends_on` edges record all 6.
  - Each of the three wire stories reads the `llm_core` additions from `story:neutral-turn-controls` (3 reads).
  - `story:client-turns-cross-wire` reads each wire story's decoders, projections and fixtures (3 reads).
- Waves: `story:neutral-turn-controls` is in wave 1, the three wire stories are in wave 2, and `story:client-turns-cross-wire` is in wave 3. This matches your plan.
- The three wire stories do not collide with each other. Each has its own `crates/llm-*/src` and `tests`, `checks/conformance/src/<wire>.rs`, `contracts/<wire>/scenarios`, `spec/domains/<wire>.yaml` and `docs/<wire>.md`.
- None of the five bodies claims `CHANGELOG.md`, `README.md`, `docs/implementation-status.md` or `docs/design.md`.

**What I could not establish:**
- `crates/llm-responses/src/request.rs:202` is the one non-test `TurnRequest` literal. Its body now names it, but the scope list for `story:neutral-turn-controls` still omits it. This is not a collision, because `depends_on` orders neutral before responses.
- The inferred scopes, `checks/conformance/src` and `crates/llm-tool-call/src/lib.rs` on `story:neutral-turn-controls` and `checks/conformance/Cargo.toml` on `story:client-turns-cross-wire`, are unverified. No collision rests on them. `checks/conformance/Cargo.toml` already lists all three protocol crates and `llm-routing`, and `checks/conformance/tests/` does not exist yet.
- Out of my lane, for the acceptance critic: the wire stories' "passing on the merged wave" lines read coordinator-owned state, so no branch can check them alone.

```findings
[]
```
