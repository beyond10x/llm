---
format: aep.planning-md/3
id: story:bedrock-event-stream
kind: story
status: draft
title: AWS event-stream frames decode, bounded, to the recorded Bedrock answers
summary: Frame decoder with CRC checks matching the smithy test data and recorded Bedrock frames
relations:
- decomposes: epic:bedrock
- serves: vision:portable-model-inference
revision: 2
---
## Change

`llm_bedrock::eventstream`: an incremental decoder for `application/vnd.amazon.eventstream`
(prelude, prelude CRC32, typed headers, payload, message CRC32), bounded per frame and per
stream, and an encoder for fixtures.

## Acceptance

- `crates/llm-bedrock/tests/eventstream.rs` decodes each valid file under
  `tests/data/eventstream/` to its headers and payload and refuses each invalid one, as the
  smithy-rs test data states.
- Every recorded Bedrock body under `tests/data/frames/*.bin` decodes whole, at every split of
  its bytes into two chunks, with no bytes left over.
