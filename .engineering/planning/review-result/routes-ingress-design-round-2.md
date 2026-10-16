---
format: aep.planning-md/3
id: review-result:routes-ingress-design-round-2
kind: review-result
status: active
title: Design critic, round 2 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
- reviews: release-plan:llm-0-6-0
revision: 1
---
approve

What I read: 5 items (the three stories, epic:routes-and-ingress, release-plan:llm-0-6-0) whole via `aep plan artifact show`. I also ran `aep plan artifact relations`, `graph`, `validate` (reports `valid`) and `waves`, and read the four round-1 review-results. In the worktree I read `spec/domains/routes.yaml`, AGENTS.md "Generated files" and "Release", CHANGELOG.md, and release-plan:llm-0-3-1, 0-4-0, 0-5-0 and 0-5-1 as precedent.

- **Edges:** I walked 12 declared edges touching the set (6 story decomposes/serves, 2 epic, 4 release-plan `delivers`). I read the graph store-wide, so I also looked outside the set. There is no `depends_on` or `blocks` edge, so there is no cycle and no chain.
- **Split abstraction:** none. The cut is still by capability. `llm-models` routes, the Responses encoder and the Messages encoder each have their own crate, round-trip tests and conformance domain. Round 1's design approve still holds.
- **Round-1 shared-file findings:** all three stories now say which files they share (`contracts/ess-inputs.yaml`, `contracts/baseline.json`). They also say the coordinator regenerates `suite.json` and `schema/` and sets the baseline once, and that conformance is read on the merged wave. This puts the cost on the coordinator rather than on an ordering edge, which is the option I named in round 1.
- **Acceptance reads traced to a producer in the set:** 1, with 0 recorded by a `depends_on` edge. The release-plan's Execution and Completion evidence read the merged wave of the three stories, and `delivers` edges run to all of them. I did not raise this. Execution says "After the wave merges into `main`", every earlier release-plan (0-3-1, 0-4-0, 0-5-0, 0-5-1) records its stories with `delivers` only, and a release-plan does not appear in `aep plan artifact waves`, so nothing can schedule it beside its producers. The three story acceptances read only their own new scenario directories, their own crate tests and decoders that already exist.

What I could not establish (all out of my lane, none set the verdict):
- The release-plan's Execution "dates the CHANGELOG section". AGENTS.md "Documentation" says a wave updates CHANGELOG `[Unreleased]`, `docs/implementation-status.md` and the affected pages in the same change. None of the three stories' scopes or bodies name those files, nor the "routes page" that story:routes-document says documents the default capabilities. Scope and parallel-safety critics own this.
- story:routes-document's acceptance has a scenario for a mapping route that "answers with those capabilities in the expanded catalog". The `llm.routes.Loading` entity in `spec/domains/routes.yaml` has no capabilities field (`routes`, `providers`, `answers`, `catalog_serving_models` only). The story allows refining the spec "only if the adapter needs it". The acceptance critic owns this.
- `aep plan artifact waves` places story:responses-ingress-encoder in wave 2 and the other two in wave 1. I did not establish why, since that is parallel-safety.

```findings
[]
```
