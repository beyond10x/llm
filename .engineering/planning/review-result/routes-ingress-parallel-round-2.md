---
format: aep.planning-md/3
id: review-result:routes-ingress-parallel-round-2
kind: review-result
status: active
title: Parallel-Safety critic, round 2 (epic:routes-and-ingress)
relations:
- reviews: epic:routes-and-ingress
- reviews: story:routes-document
- reviews: story:responses-ingress-encoder
- reviews: story:messages-ingress-encoder
- reviews: release-plan:llm-0-6-0
revision: 1
---
needs-revision

story:routes-document — its "Shared files" list omits `CHANGELOG.md` and `docs/implementation-status.md`, which `AGENTS.md` makes every wave that changes a crate or command update under Unreleased, so the siblings' entries collide there; the body has to say who writes them (the coordinator at integration, or an ordering edge). Inferred from the repo rule, not from a body — AGENTS.md:121-123, .engineering/planning/story/routes-document.md:98

story:responses-ingress-encoder — same unnamed collision with story:routes-document and story:messages-ingress-encoder on `CHANGELOG.md` and `docs/implementation-status.md`, left out of its "Shared files" section. Inferred from the repo rule — AGENTS.md:121-123, .engineering/planning/story/responses-ingress-encoder.md:53

story:messages-ingress-encoder — same unnamed collision with story:routes-document and story:responses-ingress-encoder on `CHANGELOG.md` and `docs/implementation-status.md`, left out of its "Shared files" section. Inferred from the repo rule — AGENTS.md:121-123, .engineering/planning/story/messages-ingress-encoder.md:52

Round 1 is fixed. Each story now has a "Shared files" section naming `contracts/ess-inputs.yaml`, the sibling stories, the coordinator-owned `suite.json`, `schema/` and `baseline.json`, and that conformance is read on the merged wave (routes-document.md:98, responses-ingress-encoder.md:53, messages-ingress-encoder.md:52).

**What you read.**
- 4 items, plus the round-1 record `review-result:routes-ingress-parallel-round-1`.
- Commands: `aep plan artifact show`, `waves --kind story --status draft`, `graph`, `relations`.
- Also `AGENTS.md`, `CHANGELOG.md`, `checks/conformance/src/{main,target}.rs`, and `git grep` for `EncodeIngress` and `ess-inputs`.

Surfaces:
- Cited: 3 of 3 stories, through typed scope entries and bodies.
- Inferred: 0. The `CHANGELOG.md` and `implementation-status.md` claim above is inferred, because no body or scope entry names those files.
- Unplaceable: 0.
- `waves` shows the three stories together in wave 1 with no collision among them and 0 unassessed.

`release-plan:llm-0-6-0` touches only post-merge release files and collides with nothing.

Acceptance reads traced to a producer in the set: 0. The encoder acceptances read decoders that already exist. The conformance acceptances read coordinator output, not another story's. No `depends_on` edge is missing.

**What you could not establish.**
- The three stories' typed scope omits `contracts/ess-inputs.yaml` and `Cargo.lock`, so `waves` cannot show that shared file as a collision. The bodies do name it and give the coordinator's resolution, so I did not raise it as a finding.
- The `waves` collisions of `story:credentials-from-secrets` with `story:routes-document` on `checks/conformance/Cargo.toml` involve an item outside my set. I did not judge them.
- I did not run the conformance check.

```findings
- file: .engineering/planning/story/routes-document.md
  line: 98
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Shared files section omits CHANGELOG.md and docs/implementation-status.md, which AGENTS.md says every wave that changes a crate or command updates, so this story and its two siblings collide there; the body must say who writes them (surface inferred from the AGENTS.md rule, not from a body)"
- file: .engineering/planning/story/responses-ingress-encoder.md
  line: 53
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Shared files section omits CHANGELOG.md and docs/implementation-status.md, which AGENTS.md says every wave that changes a crate or command updates, so this story collides with story:routes-document and story:messages-ingress-encoder there (surface inferred from the AGENTS.md rule, not from a body)"
- file: .engineering/planning/story/messages-ingress-encoder.md
  line: 52
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Shared files section omits CHANGELOG.md and docs/implementation-status.md, which AGENTS.md says every wave that changes a crate or command updates, so this story collides with story:routes-document and story:responses-ingress-encoder there (surface inferred from the AGENTS.md rule, not from a body)"
```
