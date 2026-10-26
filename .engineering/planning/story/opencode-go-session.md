---
format: aep.planning-md/3
id: story:opencode-go-session
kind: story
status: implemented
title: The Messages client sends x-opencode-session to an OpenCode Go endpoint
summary: Base-URL trigger; one minted session per client, or the caller's; no configurable header
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: checks/conformance/src/messages.rs
- confidence: cited
  path: contracts/anthropic/scenarios
- confidence: cited
  path: contracts/baseline.json
- confidence: cited
  path: contracts/ess-inputs.yaml
- confidence: cited
  path: contracts/schema
- confidence: cited
  path: contracts/suite.json
- confidence: cited
  path: crates/llm-messages/src/client.rs
- confidence: cited
  path: crates/llm-messages/src/client/tests.rs
- confidence: cited
  path: crates/llm-messages/src/lib.rs
- confidence: cited
  path: crates/llm-messages/tests
- confidence: cited
  path: crates/llm-messages/tests/adversary_w19_opencode_session.rs
- confidence: cited
  path: crates/llm-messages/tests/opencode_session.rs
- confidence: cited
  path: crates/llm-messages/tests/wire_names.rs
- confidence: cited
  path: docs/messages.md
- confidence: cited
  path: docs/verification/messages-falsification.json
- confidence: cited
  path: spec/domains/messages.yaml
revision: 20
transitions:
- {from: "draft", to: "proposed", at: "2026-10-26T08:47:33Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-26T08:47:34Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-26T09:23:49Z", actor: "human:timo", revision: 15, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":1}}}
---
## Context

OpenCode Go answers llm's Messages requests with HTTP 400 before any model runs. The body a
takt-trial loopback probe received on 2026-10-24, as relayed by session `b10x-chef-orchestrator`:

```json
{"type":"error","error":{"type":"MissingSessionID","message":"Request is missing x-opencode-session and cannot be routed efficiently. Please see https://opencode.ai/docs/go/#where-can-i-use-it"}}
```

The Messages client sets `content-type`, `accept`, `anthropic-version` and, when one is due,
`anthropic-beta`, next to the account's authentication (`crates/llm-messages/src/client.rs`,
`MessagesClient::run`). It sends no `x-opencode-session`.

OpenCode Go's page (<https://opencode.ai/docs/go/>, read 2026-10-26), section "Where can I use
it?", says a client should "Send a stable session ID in `x-opencode-session` for each conversation
so we can optimize routing and prompt caching". It gives no format, length or character set. Its
Anthropic Messages endpoint is `https://opencode.ai/inference/go/anthropic/v1/messages`, the
base URL takt's trial endpoint names (`takt` `.engineering/planning/approval-record/trial-harness-arm-endpoint.md`).

## Change

A named behaviour of the Messages wire, not a configurable header:

- **Trigger: the endpoint's base URL.** A binding whose base URL is scheme `https`, host
  `opencode.ai`, the default port and a path under `/inference/go/` is OpenCode Go. Nothing else
  is: not `http`, not another host or a host that merely starts with `opencode.ai`, not an explicit
  port, not another path on `opencode.ai`. Chosen over an explicit endpoint option because takt's
  endpoint file (takt 0.11.1: `base_url`, `wire`, `model`, `credential`) can already express it and
  needs no new field, and a routes document or catalog needs none either.
- **Value: one session per client.** A `MessagesClient` for an OpenCode Go binding mints one
  printable-ASCII identifier at construction and sends it as `x-opencode-session` on every request
  it makes, every turn and every attempt. Two clients mint two different identifiers. A caller
  that serves one conversation per client and already names it may pass that name instead
  (`with_session`); the name must be a valid `llm_core::Id`.
- **No other endpoint changes.** A client for any other base URL sends exactly the headers it sends
  today.
- **No generic surface.** No configuration, routes field or turn field adds an arbitrary request
  header; the header name is a constant of the Messages crate.

## Out of scope

- The OpenAI Responses and Chat Completions wires on OpenCode Go: no refusal was observed there.
- The page's other request, a client user agent of its own (`my-coding-agent/1.0`); llm sets none
  today.
- Using a turn's `prompt_cache_key` hint as the session.
- A live call to OpenCode Go in the gate.

## Acceptance

- Conformance scenarios in `contracts/anthropic/scenarios/`, listed in `contracts/ess-inputs.yaml`:
  - `an-opencode-go-endpoint-sends-one-session-per-client`: the OpenCode Go base URL yields an
    `x-opencode-session` header whose value is the same on two requests of one client and differs
    between two clients;
  - `a-named-session-is-sent-as-given`: a client given a session sends that exact value;
  - `another-endpoint-sends-no-opencode-session`: `https://api.anthropic.com/v1/` yields none;
  - `look-alike-endpoints-are-not-opencode-go`: `http://opencode.ai/inference/go/anthropic/v1`,
    `https://opencode.ai.example/inference/go/anthropic/v1`, `https://opencode.ai:8443/inference/go/anthropic/v1`
    and `https://opencode.ai/zen/v1` each yield none.
- The existing `llm.messages.Present` scenarios keep their exact header lists.
- A crate test shows the header map `MessagesClient` sends for a request is the one the
  conformance observation reads.
- `RUSTUP_TOOLCHAIN=1.98.0 task check` passes.
