---
format: aep.planning-md/3
id: story:live-provider-captures
kind: story
status: implemented
title: A live Codex Responses stream and a live Messages turn with cache figures are pinned as fixtures
relations:
- serves: vision:portable-model-inference
- decomposes: epic:access
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-21T08:56:08Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-21T08:56:08Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-21T09:54:38Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":2}}}
---
## Context

Every Responses and Messages stream fixture in this repository is authored to a documented shape;
none is a recorded server answer. `docs/verification/messages.md` says the fixtures establish no
live Anthropic access, and `crates/llm-responses/tests/codex_stream.rs` reproduces the shape of a
2026-10-04 probe with synthetic identifiers and text. The owner approved two live recordings
(relayed by session `b10x-chef-orchestrator`, 2026-10-21): a Responses stream from the Codex
backend through the operator's Codex login, and a Messages turn with its cache figures through the
operator's Claude subscription token. Both credentials are already on this machine: the Codex
login at `~/.codex/auth.json`, the subscription token in the keychain under `llm/anthropic-subscription`
(`docs/live-qualification.md`). No paid API key and no new credential is used.

## Outcome

Two recorded response streams, scrubbed, pinned under the crates that decode them, and replayed by
tests that drive the real decoders over the recorded bytes.

## Acceptance

- `crates/llm-responses/tests/fixtures/live/` holds one Codex Responses stream recorded through
  `ResponsesClient` with the Codex login, and a test replays it over a loopback socket through
  `ResponsesClient`, asserting the streamed text, the terminal stop reason and the usage counters
  the recording carries.
- `crates/llm-messages/tests/fixtures/live/` holds two Messages streams recorded by the
  `live_subscription_turn` example over the subscription token, the second repeating the first
  prompt so the cache is read: the first reports a cache write, the second a cache read, and a test
  replays both through the Messages decoder and asserts those figures.
- Each fixture directory has a README naming the date, the model, how it was recorded and every
  value the scrub replaced. No token, account id, email, account-identifying request id or
  organization name is in any committed byte.
- `task check` exits 0.

## Scope

- `crates/llm-responses/tests/fixtures/live/`, `crates/llm-responses/tests/live_capture.rs`
- `crates/llm-messages/tests/fixtures/live/`, `crates/llm-messages/tests/live_capture.rs`
- `docs/verification/responses.md`, `docs/verification/messages.md`, `CHANGELOG.md`
