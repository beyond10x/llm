---
format: aep.planning-md/2
id: approval-record:wave-1-proposal
kind: approval-record
status: draft
title: Wave 1 selection authorised by an approved plan that named its commits
tags:
- plan-approved
relations:
- decides: story:messages-projection
- decides: story:responses-projection
- decides: story:chat-projection
- decides: story:gateway-auth
- decides: story:hosting-contract
- decides: story:public-surface
revision: 2
---
## The stop

The wave skill's stage-1 proposal stop, for wave 1 of this repository.

## What authorises passing it

The operator read and approved a written plan for this work on 2026-09-19, in the session that wrote
this record. That plan named the six units below, the partition of surfaces between them, and the
exact list of commits the wave would make. A plan that names its commits and is approved authorises
those commits and no others.

The operator separately answered three questions in the same session: the wave integrates into
`plan/llm-foundation` and merges back to it; the branch is pushed to origin after the wave closes so
the pull request's checks run; and at most four agents run at once.

## The units, and the objective each serves

Every one serves `vision:portable-model-inference`, the only objective this store declares.

- `story:messages-projection` — Anthropic Messages codec, decoder and client.
- `story:responses-projection` — Responses codec, decoder and client.
- `story:chat-projection` — Chat Completions codec, decoder and client.
- `story:gateway-auth` — the authenticated single-owner gateway, without protocol translation.
- `story:hosting-contract` — owned-resource lifecycle against a fake provider.
- `story:public-surface` — documentation, changelog, licence, gating and the Atlas catalog wiring.

## What the store computed

`aep plan artifact waves --kind story --status active` returns one wave holding all six, with zero
collisions and zero unassessed stories. `story:runtime-contracts` appears in that wave and is not
dispatched: it is the umbrella contract story, and it stays active until the wave's own evidence is
recorded against it.

Three collisions existed before this record and were resolved by moving a surface to its real owner
rather than by ignoring them: `AGENTS.md`, `README.md` and `.github/workflows` left
`story:runtime-contracts` for `story:public-surface`.

`story:connectors-secret-resolver` was excluded. It is blocked by
`dependency-blocker:connectors-arbitrary-secrets`, and a blocked story leaves the set. Its exclusion
is also what makes `story:hosting-contract` admissible in this wave: the two share
`spec/domains/catalog.yaml`.

## How the surfaces were established

The plan dispatched six `aep-drive:story-scoper` agents, one per unit, read-only. The run was
interrupted by an account usage limit partway through and resumed automatically after the limit
reset; all six returned. Their sections are now each artifact's `## Scope` section, and their typed
entries are what the wave computation above reads.

An earlier revision of this record said the scopers had been lost and that the coordinator had
written the entries in their place. That was written while the run was still paused and was wrong.
The coordinator's own partition, produced before the scopers returned, is what the opening commit
pre-wired; the scopers then reached the same partition independently and named the four shared
registration files as the reason it would not hold without that pre-wiring. Both readings agree.

The correction is recorded here rather than made silently, because a scope nobody established reads
exactly like one that was, and so does a claim that nobody checked.
