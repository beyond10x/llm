---
format: aep.planning-md/3
id: review-result:routes-ingress-parallel-round-1
kind: review-result
status: active
title: Parallel-safety critic, round 1 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
revision: 1
---
needs-revision

story:routes-document — all three stories append scenario paths to the hand-edited `contracts/ess-inputs.yaml` and each acceptance says `contracts/baseline.json` is "raised by their count". The body names neither the other two stories nor who writes these files. The coordinator's list of post-merge files omits `ess-inputs.yaml`, and `task conformance` also reads `contracts/suite.json` and `contracts/schema`, which the coordinator regenerates after merge. The body has to say which of these it edits and that its conformance acceptance is read on the merged wave. The surface is cited from the bodies. The sibling names are inferred from the other two bodies — .engineering/planning/story/routes-document.md:76-77; checks/conformance/src/gate.rs:70-100

story:responses-ingress-encoder — same shared-file collision with story:routes-document and story:messages-ingress-encoder on `contracts/ess-inputs.yaml` and `contracts/baseline.json`, and its `task conformance` acceptance reads projections the coordinator regenerates. The body says none of this, so it needs the same statement — .engineering/planning/story/responses-ingress-encoder.md:55-57

story:messages-ingress-encoder — same shared-file collision with story:routes-document and story:responses-ingress-encoder on `contracts/ess-inputs.yaml` and `contracts/baseline.json`, and its `task conformance` acceptance reads projections the coordinator regenerates. The body says none of this, so it needs the same statement — .engineering/planning/story/messages-ingress-encoder.md:54-56

For each of the three, the remedy is either an ordering edge that records the shared file as its reason, or splitting the shared surface so the items no longer share it. For example, the coordinator could own `ess-inputs.yaml` and `baseline.json` outright. I am not choosing between them.

**What you read.** I read 4 artifacts (3 stories plus the epic) with `aep plan artifact show`, `aep plan artifact graph`, `aep plan artifact relations` and `aep plan artifact waves`. I also read `spec/domains/routes.yaml`, `AGENTS.md`, `checks/conformance/src/{main,target,gate}.rs`, `contracts/{baseline.json,ess-inputs.yaml}` and the crate layouts.

Surface counts:
- **Cited:** 3 of 3 items name concrete paths or crates.
  - `routes-document` names `crates/llm-models` (module `routes`, tests), `crates/llm-routing` (a `CatalogDocument` constructor) and `contracts/routes/scenarios/`.
  - `responses-ingress-encoder` names `crates/llm-responses/src` and `contracts/responses/scenarios/`.
  - `messages-ingress-encoder` names `crates/llm-messages/src` and `contracts/messages/scenarios/`.
- **Inferred:** the spec and adapter files that a new command needs.
  - For the two encoders these are `spec/domains/{responses,messages}.yaml` and `checks/conformance/src/{responses,messages}.rs`, inferred from the Chat precedent.
  - For `routes-document` these are `checks/conformance/src/{main,target,routes}.rs`, `checks/conformance/Cargo.toml` and `Cargo.lock`.
  - None of these files is shared between two items, so no finding rests on them.
- **Unplaceable:** 0.

Acceptance reads traced to a producer in the set: 0 of 0. Every round-trip acceptance reads a decoder or `ingest_request` that already exists in the tree, so no `depends_on` edge is missing.

**What I could not establish.**
- `aep plan artifact waves` lists all three items as `unassessed` because the typed scope entries are not yet written. I judged from the bodies alone, and the scope entries should be re-checked once written.
- Out of my lane, for the acceptance critic: the three acceptances rely on `task conformance`, which cannot go green inside a single story branch while the coordinator regenerates `suite.json`, `schema` and `baseline.json` after merge.
- Out of my lane, for the design or acceptance critics: the example in `spec/domains/routes.yaml` shows the `gateway` provider with `auth: {kind: anonymous}`, while story:routes-document shows a `file:` bearer secret.
- Collisions with items outside the set were not examined.

```findings
- file: .engineering/planning/story/routes-document.md
  line: 76
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "all three stories append to the hand-edited contracts/ess-inputs.yaml and each acceptance raises contracts/baseline.json, with no mention of the others; ess-inputs.yaml is not among the coordinator's post-merge files, and the conformance acceptance reads suite.json/schema the coordinator regenerates, so the body must say which shared files it edits and that conformance is read on the merged wave (surface cited from the bodies, sibling collision inferred from them)"
- file: .engineering/planning/story/responses-ingress-encoder.md
  line: 55
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "shares contracts/ess-inputs.yaml and contracts/baseline.json with story:routes-document and story:messages-ingress-encoder, and its task conformance acceptance reads projections the coordinator regenerates after merge; the body says neither"
- file: .engineering/planning/story/messages-ingress-encoder.md
  line: 54
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "shares contracts/ess-inputs.yaml and contracts/baseline.json with story:routes-document and story:responses-ingress-encoder, and its task conformance acceptance reads projections the coordinator regenerates after merge; the body says neither"
```
