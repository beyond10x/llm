---
format: aep.planning-md/3
id: epic:routes-and-ingress
kind: epic
status: implemented
title: Shared model routes and Responses and Messages ingress answers
summary: One llm.routes/1 document read alike by gateway, CLI and SDK; Responses and Messages ingress encoders; released as 0.6.0
relations:
- decomposes: initiative:llm-foundation
- serves: vision:portable-model-inference
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"review_outcome":1}}}
- {from: "proposed", to: "active", at: "2026-10-16T02:36:56Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"review_outcome":1}}}
- {from: "active", to: "implemented", at: "2026-10-16T16:08:25Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"review_outcome":1}}}
---
## Outcome

A gateway, an agent CLI and an SDK configure model reach from one short `llm.routes/1` document read
the same way everywhere, and a server that answers callers on the OpenAI Responses or the Anthropic
Messages wire turns a neutral turn back into that wire with this library, as it already can for Chat
Completions (`crates/llm-chat/src/ingress.rs`). A route may carry its own capabilities, since
capabilities gate every request. Released as llm 0.6.0.

## Done when

- `story:routes-document`, `story:responses-ingress-encoder` and `story:messages-ingress-encoder`
  are implemented on their acceptance evidence;
- `RUSTUP_TOOLCHAIN=1.98.0 task check` passes on the merged wave, with `contracts/baseline.json`
  raised by the new scenarios and nothing skipped;
- release 0.6.0 is tagged and published as AGENTS.md "Release" describes; `release-plan:llm-0-6-0`
  claims it.

## Out of scope

- The gateway, Loom CLI and Loom SDK changes that read the document (their repositories).
- Serving: no HTTP server is added here (AGENTS.md "Owns, and does not own").
- Subscription OAuth accounts in `llm.routes/1`: the catalog keeps them; the routes document refuses
  the kind by name.

## Evidence

Operator-approved plan, 2026-10-16 (Milestone 1: llm 0.6.0). The noun `llm.routes/1` is modelled
first in `spec/domains/routes.yaml` (`ess specify validate --path spec`: `llm v1 — 14 file(s),
valid`, ess 0.59.0).

Closed 2026-10-16 on the evidence of each line of "Done when":

- `story:routes-document`, `story:responses-ingress-encoder` and `story:messages-ingress-encoder`
  are `implemented`, merged in https://github.com/beyond10x/llm/pull/33.
- The merged wave's `Gate` run (`task check` on Rust 1.98.0) passed on the release commit
  `74921f7c`: https://github.com/beyond10x/llm/actions/runs/38920897319.
- Release 0.6.0 is tagged on `74921f7c` and published:
  https://github.com/beyond10x/llm/releases/tag/0.6.0 (https://github.com/beyond10x/llm/pull/34),
  with `Shared source gates` and `Documentation validation` green on that commit.
