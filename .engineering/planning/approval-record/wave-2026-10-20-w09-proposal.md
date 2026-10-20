---
format: aep.planning-md/3
id: approval-record:wave-2026-10-20-w09-proposal
kind: approval-record
status: draft
title: Wave 2026-10-20-w09 authorised by the owner's approval of its proposal
tags:
- owner-approved
relations:
- decides: story:capabilities-declare-streaming
- decides: story:harness-cutover-on-llm-0-7
- decides: story:text-deltas-are-final
- decides: story:tool-result-rejected-or-failed
revision: 1
---
## The stop

The wave skill's stage-1 proposal stop for wave 2026-10-20-w09.

## What authorises passing it

The owner approved the proposal in this repository's coordinating session on 2026-10-20 ("I approve
it"), after reading the proposal recorded in `docs/plan/wave-2026-10-20-w09.md`. The approval covers
the commits that page names: the opening store commit; one commit or a short series per unit on its
`impl/<story>` branch; the merges into `wave/2026-10-20-w09`; the integration commit; the closing
store commit; bot pushes of the wave branch, the wave pull request and its merge into `main` once
`Gate` and `Shared source gates` are green. All as `b10x-bot[bot]` through `b10x-gates`. Nothing
else: no tag, no release, no other repository.

## The units, and the objective each serves

All four serve `vision:portable-model-inference`.

- `story:capabilities-declare-streaming` (layer 1) — `Capabilities` declares output streaming and
  input during output.
- `story:harness-cutover-on-llm-0-7` (layer 1) — the Harness parity record and status current to
  0.7.0 for the cutover Atlas ADR 0097 decided.
- `story:text-deltas-are-final` (layer 2) — a text delta is final, and a caller is told when the
  outcome's text differs from the deltas shown.
- `story:tool-result-rejected-or-failed` (layer 3) — a tool result tells a rejected call from a
  failed one.

## Left out

`story:input-into-a-running-turn` (depends on `story:capabilities-declare-streaming`) and
`story:crates-from-a-registry` (blocked by `decision-blocker:registry-publication`).
