---
format: aep.planning-md/3
id: story:orphan-termination-obligations
kind: story
status: archived
title: Orphan terminations record and discharge a stop obligation
relations:
- serves: vision:portable-model-inference
- decomposes: epic:hosting
revision: 3
transitions:
- {from: "draft", to: "archived", at: "2026-10-05T10:17:39Z", actor: "human:timo", revision: 3}
---
## Acceptance

Filed from wave 3 (2026-09-27) to own a `DEFERRED:` note in `spec/domains/runpod.yaml`. The
acceptance is written when the story is scoped.

## Moved

Moved to `beyond10x/llm-gateway` as `story:orphan-termination-obligations` on 2026-10-05 by story:serving-extraction, with
the crates it describes (llm-gateway `be722b4`). Archived here; the work continues there. Its
evidence records stay in this store.
