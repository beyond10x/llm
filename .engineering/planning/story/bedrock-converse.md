---
format: aep.planning-md/3
id: story:bedrock-converse
kind: story
status: implemented
title: A turn runs on Bedrock Converse and ConverseStream
summary: Converse wire, BedrockClient behind Model::turn, Protocol bedrock-converse, byte-stream transport, exception classes
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
- depends_on: story:bedrock-signing
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-22T21:52:24Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-22T21:52:24Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-22T21:52:24Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}}
---
## Change

- `Protocol::BedrockConverse` (`bedrock-converse`) in `llm-core`.
- `llm-http`: a raw byte-stream POST for a named media type, and a JSON GET for credential
  endpoints.
- `BedrockTarget`, `BedrockClient` (`Model`), the Converse encoder and stream decoder,
  `/converse` and `/converse-stream`, and the class of each Bedrock exception.

## Acceptance

- Each recorded ConverseStream body under `tests/data/frames/` replayed through a local fixture
  server yields the text, tool calls, reasoning, stop reason and usage it carries; a counter it
  does not report stays unknown.
- A `throttlingException` frame before output is retriable `rate-limited`; after output it is
  final; `validationException` is final.
- A cut after output is final; a cut before any output stays retriable.
- The request a fixture server receives carries a SigV4 `authorization` header for service
  `bedrock` and the target's region.
