---
format: aep.planning-md/3
id: review-result:client-fields-parallel-safety-round-1
kind: review-result
status: active
title: Parallel-safety critic, round 1, epic:client-request-fields
relations:
- reviews: story:neutral-turn-controls
- reviews: story:messages-ingress-client-fields
- reviews: story:responses-ingress-client-fields
- reviews: story:chat-ingress-closed-levels
- reviews: story:client-turns-cross-wire
revision: 1
---
needs-revision
story:messages-ingress-client-fields — its Shared files section has the branch edit "its own lines of `contracts/ess-inputs.yaml`", but the plan gives that file to the coordinator, and the file is not in the story's scope. Three parallel branches would each edit one manifest. Fix the body to say the coordinator lists the scenarios (as it already does for `contracts/suite.json`), or record an ordering reason for the shared file (cited) — .engineering/planning/story/messages-ingress-client-fields.md:62
story:responses-ingress-client-fields — its Shared files section has the branch edit "its own lines of `contracts/ess-inputs.yaml`", but the plan gives that file to the coordinator, and the file is not in the story's scope. Same pair of fixes as above (cited) — .engineering/planning/story/responses-ingress-client-fields.md:70
story:chat-ingress-closed-levels — its Shared files section has the branch edit "its own lines of `contracts/ess-inputs.yaml`", but the plan gives that file to the coordinator, and the file is not in the story's scope. Same pair of fixes as above (cited) — .engineering/planning/story/chat-ingress-closed-levels.md:66

Why this is a finding: the conformance gate refuses any scenario the manifest does not list (`checks/conformance/src/gate.rs:75-96`, `AGENTS.md:96-100`). So each wire branch needs the file edited to run its own check. If the coordinator owns it, each body has to say how the branch gets its scenarios listed.

**What I read:** 5 stories, plus the epic body.
- Commands: `aep plan artifact show` on each of the five, `aep plan artifact graph`, and `aep plan artifact waves --kind story --status draft --format json`.
- Tree reads: `checks/conformance/src/{main,target,gate}.rs`, `contracts/ess-inputs.yaml`, `AGENTS.md`, and a `git grep` for `TurnRequest {`.
- Surfaces: 3 stories cited fully (messages, responses, chat). 1 is cited in its main paths and inferred in the rest (neutral-turn-controls). 1 is cited on its test file and inferred on `checks/conformance/Cargo.toml` (client-turns-cross-wire). Unplaceable: 0.
- Acceptance reads traced to a producer in the set: 6 of 6, and `depends_on` records all 6.
  - Each wire story reads `llm_core` additions from neutral-turn-controls (3 reads).
  - client-turns-cross-wire reads each wire story's decoders and fixtures (3 reads).
- The waves output puts neutral-turn-controls in wave 1, the three wire stories in wave 2, and client-turns-cross-wire in wave 3. That matches your plan.
- The wire stories do not collide on the other files:
  - Each owns its own `crates/llm-*/src` and `tests`, its own `checks/conformance/src/<wire>.rs`, `contracts/<wire>/scenarios`, `spec/domains/<wire>.yaml` and `docs/<wire>.md`.
  - `checks/conformance/src/target.rs` dispatches through each module's own `observe` and `VIEWS`, so no shared edit there.

**What I could not establish:**
- Not a collision, so not a finding: `crates/llm-responses/src/request.rs:202` builds a non-test `TurnRequest` literal. neutral-turn-controls names only tests ("Every crate's tests", line 71) and does not scope that file. It is ordered before the responses story by `depends_on`, so it does not collide. Its body should still say it edits that source file.
- Out of my lane, for the acceptance critic: the wire stories' "passing on the merged wave" conformance lines read `contracts/suite.json`, `contracts/schema` and `contracts/baseline.json`. The coordinator regenerates those after the merge, so no branch can check the line on its own.
- Inferred scopes I did not verify: `checks/conformance/src` and `crates/llm-tool-call/src/lib.rs` on neutral-turn-controls, and `checks/conformance/Cargo.toml` on client-turns-cross-wire. No collision depends on them.

```findings
- file: .engineering/planning/story/messages-ingress-client-fields.md
  line: 62
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Shared files section has the branch edit its own lines of contracts/ess-inputs.yaml, which the plan gives to the coordinator and the scope omits; the same file is claimed by the two sibling wire stories (cited), so name the coordinator as owner or record an ordering edge with the file as its reason"
- file: .engineering/planning/story/responses-ingress-client-fields.md
  line: 70
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Shared files section has the branch edit its own lines of contracts/ess-inputs.yaml, which the plan gives to the coordinator and the scope omits; the same file is claimed by the two sibling wire stories (cited), so name the coordinator as owner or record an ordering edge with the file as its reason"
- file: .engineering/planning/story/chat-ingress-closed-levels.md
  line: 66
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Shared files section has the branch edit its own lines of contracts/ess-inputs.yaml, which the plan gives to the coordinator and the scope omits; the same file is claimed by the two sibling wire stories (cited), so name the coordinator as owner or record an ordering edge with the file as its reason"
```
