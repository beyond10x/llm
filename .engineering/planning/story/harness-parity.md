---
format: aep.planning-md/3
id: story:harness-parity
kind: story
status: implemented
title: A cited matrix shows llm covers every capability of Harness's model crates
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
scope:
- confidence: inferred
  path: docs
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-04T22:42:44Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-04T22:42:45Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-04T23:10:52Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Outcome

A capability matrix states, for every public capability of Harness's model crates, whether llm has
it, and where. Every gap is either closed in llm or filed as its own story here.

## Harness crates compared

`harness-messages` (2,501 src lines), `harness-responses` (1,892), `harness-http` (1,934),
`harness-credential` (1,204); 7,528 in total at harness `2fd7235b`.

## Acceptance

- The matrix is committed under `docs/` in llm. One row per public item or behaviour of the four
  crates (wire types, streaming events, tool-call handling, retries, timeouts, cancellation,
  credential sources including Codex and Anthropic subscription logins, error classes, usage
  accounting).
- Each row cites the Harness `path:line` and either the llm `path:line` that covers it or the
  story id that will.
- No row is marked covered without a test in llm that exercises it, named in the row.
- Zero rows without a citation.

## Known input

The Codex stream gap found on 2026-10-04 (text deltas without `output_item.done`, recorded on
`story:codex-stream`) is one row.

## Scope (inferred)

Read-only over harness; writes `docs/` and new draft stories in llm.
