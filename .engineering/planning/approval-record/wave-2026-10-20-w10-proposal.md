---
format: aep.planning-md/3
id: approval-record:wave-2026-10-20-w10-proposal
kind: approval-record
status: draft
title: Wave 2026-10-20-w10 authorised by the owner's grant to adr-changes
tags:
- delegate-approved
relations:
- decides: story:quota-429-is-final
- decides: story:relayed-failures-keep-their-retry-decision
- decides: story:codex-renewal-test-load-independent
- decides: story:routes-refusal-test-names-its-field
- decides: story:messages-cut-after-silent-output-is-final
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-20-w10.

## What authorises passing it

Session `adr-changes` asked for this wave on 2026-10-20 (`story:relayed-failures-keep-their-retry-decision`
first, plus what fits without collision), approved the proposal in `docs/plan/wave-2026-10-20-w10.md`
("all as proposed"), chose design option D for the relayed failure, and held the wave for the
account budget. It lifted the hold on 2026-10-20 on the owner's word ("start wave 2026-10-20-w10
now (fixes + 0.8.1 …; already approved)"). The owner's standing grant of 2026-10-20 makes that
session's approvals for work implementing Atlas ADRs 0096 to 0101 count as the owner's; the five
stories are follow-ups of wave 2026-10-20-w09, which carried the Harness cutover of ADR 0097 and
the inference gaps of ADR 0100.

The approval covers the commits the wave page names: the opening store commit; one commit per story
on its unit branch; the merges into `wave/2026-10-20-w10`; the integration commit; the closing store
commit; bot pushes of the wave branch, the wave pull request and its merge into `main` once `Gate`
and `Shared source gates` are green; then release `0.8.1` through AGENTS.md "Release", which Loom,
Harness and inference pin. All as `b10x-bot[bot]` through `b10x-gates`. Nothing else: no registry
publication, no other repository.

## The units, and the objective each serves

All five serve `vision:portable-model-inference`.

- `story:quota-429-is-final` (layer 1, unit http) — an HTTP 429 a provider documents as an exhausted
  quota is final.
- `story:relayed-failures-keep-their-retry-decision` (layer 1, unit http, after the quota story) —
  the HTTP transport reads a do-not-retry signal on an error status (option D).
- `story:codex-renewal-test-load-independent` (layer 1) — the Codex renewal resend test decides its
  claim under load.
- `story:routes-refusal-test-names-its-field` (layer 1) — the routes refusal test is refused for the
  field each case names.
- `story:messages-cut-after-silent-output-is-final` (layer 2) — a Messages turn cut after a silent
  block is final.

## Left out

`story:messages-input-total-derived-from-what-llm-observed` (needs evidence first),
`story:input-into-a-running-turn` (offer-or-not decision not taken),
`story:http-transport-load-sensitivity` (shares `crates/llm-http/tests/transport.rs` with unit http).
