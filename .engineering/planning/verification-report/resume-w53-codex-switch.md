---
format: aep.planning-md/3
id: verification-report:resume-w53-codex-switch
kind: verification-report
status: draft
title: Claude diagnostic work preserved; operator switches to Codex
relations:
- verifies: story:anthropic-access
revision: 1
---
## Session and disposition

Resumed Claude session 457b5cc2-de5e-4a74-ad2a-227828e4945b. The operator then instructed:
"you need to switch to codex now ...". Continue live Loom verification on the existing Codex
login. Claude subscription qualification stays blocked; no more Claude calls were made after
that instruction. No API-key substitution, release, or finished qualification is claimed.

## Preserved work

Wave 2026-10-05-w53 lives in managed tree `llm-w53-live-fields`, branch
`impl/live-messages-fields`, based on 8618681c9b3156a98ef7f71233e4c2758d4d1a33.
The uncommitted source adds safe field-path refusals and response-only capture to the live example.
Scratch/evidence is under `~/.cache/ga-wave-2026-10-05-w53`; build output is
`~/.cache/b10x-target/llm-w53`. The tree is archived before handoff and retained for resumption.

## Fresh validation

Rust 1.98.0: the live example's fixture integration suite passes 8/8. A bounded adversary pass
added two cases: field-name boundaries and head-only capture for rejected status/media types.
The transport and projection suites pass 11/11 and 21/21 respectively, compared with 10/10 and
20/20 when the two added cases are deselected. No concrete findings were returned. These tests
make only loopback calls. `git diff --check` passes.

The earlier implementor reported full gates green on ESS 0.52.0. That full gate was not rerun
in this resumed session. `b10x upgrade ess --host claude` and application of its plan confirm
0.53.0 is the latest installed release. ESS 0.53.0 validates and compiles the specification;
synthesis selects 691 scenarios with zero refusals. Its suite differs in provenance
(`scenario_initial_state: empty`, suite version 35). Repository CI and crate pins remain at
0.52.0 and must move together before publishing this work under the newest-ESS rule.

## Live observation and next action

The one Claude retry at 2026-10-05T20:42:02Z returned HTTP 429 with Retry-After 8877 seconds;
no Messages body was supplied. The credential blocker records the report path and retry time.
The original refused field is still unknown. After the delay, the owner may resume the live
capture with a fresh filename, make the observed response a deterministic regression test,
then implement and qualify the fix plus credential rotation. No retry is scheduled.
