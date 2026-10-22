---
format: aep.planning-md/3
id: story:bedrock-anthropic-invoke
kind: story
status: implemented
title: A turn runs on Anthropic models through Bedrock InvokeModel
summary: Messages body with anthropic_version bedrock-2023-05-31 on invoke and invoke-with-response-stream
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-22T21:52:24Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-22T21:52:25Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-22T21:52:25Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}}
---
## Change

The Anthropic `InvokeModel` wire: the Messages body from `llm-messages` with
`anthropic_version: bedrock-2023-05-31` and without `model` and `stream`; `chunk` frames carrying
base64 Messages events decoded through the `llm-messages` stream decoder; Bedrock's
`amazon-bedrock-invocationMetrics` read only where Messages usage is absent.

## Acceptance

- The recorded `invoke-with-response-stream` body under `tests/data/frames/` yields the turn it
  carries, usage included; the recorded `invoke` body decodes through `/invoke`.
- The encoded body for a fixture turn has `anthropic_version` `bedrock-2023-05-31` and no
  `model` or `stream` key.
