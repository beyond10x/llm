---
format: aep.planning-md/3
id: approval-record:wave-2026-10-20-w11-proposal
kind: approval-record
status: draft
title: Wave 2026-10-20-w11 authorised by the owner's grant to b10x-chef-orchestrator
relations:
- decides: story:a-resolver-for-anonymous-bindings
- decides: story:async-clients-without-a-tokio-runtime
- decides: story:messages-input-total-derived-from-what-llm-observed
- decides: story:input-into-a-running-turn
- decides: story:a-llama-server-response-fixture
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-20-w11.

## What authorises passing it

Session `b10x-chef-orchestrator` (formerly `adr-changes`) approved the proposal in
`docs/plan/wave-2026-10-20-w11.md` as proposed on 2026-10-20, under the owner's standing grant
(`~/.claude/CLAUDE.md`, exception 5): its approvals for work implementing Atlas ADRs 0096 to 0101
count as the owner's. Four stories cite ADR 0100 and one ADR 0098, as that session checked.

Covered: the commits the wave page lists; bot pushes of the wave branch; the wave pull request and
its landing on `main` by a bot fast-forward of the green head (not a merge through the pull
request); release `0.8.3` through AGENTS.md "Release", since Harness, Loom and inference pin it. Not
covered: registry publishing, and `story:the-neutral-turn-in-ess` and
`story:cost-units-for-speech-and-requests`, which get their own proposal. Units run one after
another, one agent at a time; every cargo command goes through `b10x-build-slot`.

## The units, and the objective each serves

All five serve `vision:portable-model-inference`, in this order:
`story:a-resolver-for-anonymous-bindings`, `story:async-clients-without-a-tokio-runtime`,
`story:messages-input-total-derived-from-what-llm-observed`, `story:input-into-a-running-turn`,
`story:a-llama-server-response-fixture`. Each story's `## Design decisions` records the decision
the proposal named.
