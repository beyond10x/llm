---
format: aep.planning-md/3
id: story:live-diagnostics
kind: story
status: implemented
title: A live run names the refused field and can capture the response
relations:
- decomposes: epic:access
- informed_by: story:anthropic-access
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/llm-credentials/examples
- confidence: inferred
  path: crates/llm-credentials/tests
- confidence: inferred
  path: crates/llm-http
- confidence: inferred
  path: crates/llm-messages
- confidence: inferred
  path: docs
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-10-05T23:46:14Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-05T23:46:14Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-05T23:46:27Z", actor: "human:timo", revision: 11, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Outcome

A live run says what went wrong: llm-messages' "Messages field is outside the declared subset"
refusal names the field by path (names only, never values), and the live-run example can write the
raw response (status line, response headers, SSE body; never request headers or the token) to a
new mode-0600 file through an llm-http `ResponseTap`.

## Why

The operator's first live subscription turn (2026-10-05, story:anthropic-access) was accepted by
the API and refused by llm's decoder without naming the field, so it could not be fixed. Split out
of story:anthropic-access when the work grew past it: a new llm-http API, a scenario pin and the
falsification record.

## Acceptance

- The refusal names the path (`message_start.message.<name>`, `content_block_delta.delta.<name>`,
  `messages[].content[].<name>`); a name over 64 bytes or outside `[A-Za-z0-9_-]` is written `?`.
- `--capture-response <path>` refuses an existing file, writes mode 0600, holds no request header
  and no token (fixture test), and only streamed responses reach the tap.
- The live rerun of 2026-10-05T23:46Z named `message_start.message.container`.
