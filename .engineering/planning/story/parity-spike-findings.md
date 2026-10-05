---
format: aep.planning-md/3
id: story:parity-spike-findings
kind: story
status: draft
title: The parity table records the two behaviours the Harness spike found
relations:
- decomposes: epic:serving-split
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

`docs/harness-parity.md` records two behaviours the Harness spike (beyond10x/harness branch
`spike/builds-on-llm-0.1.7`, `8989d138`) found that its 146 rows do not list, each as `gap` or
deliberately different, with the decision and its test:

1. Under a forced tool choice, llm treats a turn that ends in prose as a protocol error; Harness
   read it as a prose turn (Harness test `a_run_asked_for_a_schema_that_answers_in_prose_…`,
   ignored on the spike).
2. llm-messages refuses a `signature_delta` when the thinking block opened without a `signature`
   field; Harness's fake server opens blocks that way (`a_thinking_round_trip_completes_through_the_shipped_binary`,
   ignored on the spike; beside row M30).

## Acceptance

Two new rows with citations to both sides; the counts table and `## Gaps` follow; each Harness test
ignored on the spike either passes against llm or names the row it waits on.
