---
format: aep.planning-md/3
id: story:messages-cut-after-silent-output-is-final
kind: story
status: draft
title: A Messages turn cut after a silent block is final, as on Responses
summary: The Messages client does not clear the retry mark once its decoder has answered; Responses does
relations:
- decomposes: epic:routing
- serves: vision:portable-model-inference
revision: 1
---
## Context

Found by the implementor of `story:retry-classes-harness-retried` in wave 2026-10-20-w09, with a
probe: a Messages transport cut after a silent block (opened, nothing visible streamed) stays
retriable (`cut after silent text: shown=0 code=Protocol dispatch=Accepted retriable=true`). The
Responses client clears the mark once its decoder has answered (`crates/llm-responses/src/client.rs`,
the `Err` arm of its stream loop); the Messages client does not. It has been so since wave w27.

## Outcome

A Messages turn that fails after any output, visible or silent, is final, as on Responses.

## Acceptance

- A test cuts a Messages stream after a silent block and asserts the failure is not retriable;
  a cut before any block stays retriable.
- The fix shape the implementor proposed: a `pub(crate) fn has_answered` on `StreamDecoder`, and
  `with_retriable(false)` in the `Err(error)` arm of `MessagesClient::stream`.
