---
format: aep.planning-md/3
id: story:a-llama-server-response-fixture
kind: story
status: implemented
title: The Chat Completions client is pinned against a llama-server-shaped stream
relations:
- decomposes: epic:inference
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/baseline.json
- confidence: cited
  path: contracts/chat/scenarios
- confidence: inferred
  path: contracts/ess-inputs.yaml
- confidence: inferred
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-chat/fixtures
- confidence: inferred
  path: crates/llm-chat/src/incoming.rs
- confidence: inferred
  path: crates/llm-chat/tests
- confidence: cited
  path: docs/verification/chat.md
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T15:37:10Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T15:37:10Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-20T21:51:17Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

Reported by the `inference` repository (Atlas ADR 0100), which serves models with llama-server
(llama.cpp's HTTP server) behind the Chat Completions client. `crates/llm-chat/fixtures/` holds five
response streams: two OpenAI-shaped, two vLLM-shaped and one with a single usage counter. None is in
llama-server's shape, and all five are authored to the documented shape, not captured from a live
server (`docs/verification/chat.md:40-44`). Which fields llama-server adds to a streamed chunk, and
whether the decoder refuses, carries or drops them, is not established.

## Outcome

The Chat Completions client is pinned against a llama-server-shaped stream whose source is stated.

## Acceptance

- A fixture in llama-server's streamed shape (text, a tool call if llama-server streams one, and its
  usage or timing report), either captured from a local llama-server with no paid call or authored
  from llama.cpp's own documentation or source, cited by file and commit; the fixture says which.
- A test decodes it and asserts the turn's text, tool call and usage; a field llama-server adds is
  carried or refused under the workspace rule, never dropped.
- An ESS scenario under `contracts/chat/scenarios/` carries the same bytes, and
  `docs/verification/chat.md` lists the fixture with its source.

## Design decisions

Approved with wave 2026-10-20-w11 by session `b10x-chef-orchestrator` under the owner's grant: the
fixture is authored from llama.cpp's server source at a cited commit (no llama-server is installed
on the build machine, and no paid call is made), and says so.

Taken by the coordinator during the wave:

- `timings` counters (`prompt_n`, `cache_n`, `predicted_n`) are read: they fill a counter the
  `usage` report leaves out, and must agree with it where both name one.
- The eight other documented `timings` fields (durations, rates, draft counts) are accepted and not
  read, with a shape check, following `UNCONSUMED` in `b10x-llm-messages`: refusing them would refuse
  every llama-server turn, and the neutral `Usage` has no place for durations and rates. A value in
  a shape llama-server does not write is refused by field name. This reads the acceptance's
  "carried or refused, never dropped" as satisfied by an accepted, checked, documented field.
- After adversary pass 1: a contradiction between `usage` and `timings` is refused before either is
  stored, so the refused turn reports neither count.
