---
format: aep.planning-md/3
id: approval-record:seam-stories-2026-11-05
kind: approval-record
status: approved
title: The inference seam stories, accepted by the orchestrator on 2026-11-05
relations:
- decides: story:call-records
- decides: story:closed-label-classifier
- decides: story:openai-decisions-client
- decides: story:rubric-judge
- decides: story:embed-route-kind
- decides: story:shadow-calls
- decides: story:offline-replay-eval
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T18:17:57Z", actor: "human:timo", revision: 2}
---
## Decision

Session `b10x-chef-orchestrator`, acting as operator under the owner's grant (orchestrator,
2026-11-05), took the ownership of the inference seams a downstream adopter asked about and
accepted the stories that carry them:

> 1. Closed-label classifier → llm. A label-set trait; one label with per-label confidence (basis
> points) or an abstention with a cause (timeout, provider error, off-label); a registry with a
> fallback chain that can end in a caller-supplied lexical classifier; model paths over JSON-schema
> output, forced tool, and a provider closed-label decision endpoint. Take in the existing draft
> story:openai-decisions-client. Latency and cost per call are reported.
> 2. Rubric judge → llm, same typed-decision family: one rubric call, a strict JSON verdict, model
> and rubric version pinned in the result, stable across runs.
> 3. Embeddings → llm: an embed route kind, embed(texts) → vectors, with token usage, a timeout and
> batch.
> 4. In llm, at router level: a per-call record with no text and with cost; shadow calls (a
> detached second route with a daily money cap); offline replay eval over a recorded corpus with a
> money cap. The deployed ledger stays with llm-gateway story:usage-records.

> I accept them all now: move each to proposed then active.

## Stories

- `story:call-records` (decision 4, the record)
- `story:closed-label-classifier` (decision 1, JSON-schema and forced-tool paths, chain, lexical)
- `story:openai-decisions-client` (decision 1, the decision-endpoint path; existing draft, taken in)
- `story:rubric-judge` (decision 2)
- `story:embed-route-kind` (decision 3)
- `story:shadow-calls` (decision 4)
- `story:offline-replay-eval` (decision 4)

The new nouns are drafted first as ESS domains `llm.decisions`, `llm.embeddings` and `llm.calls`
in `.engineering/specs/llm-seams/domains/`, validated with ess 0.86.0 in a copy of `spec/` with
the three added (`llm v1 — 18 file(s), valid`).
