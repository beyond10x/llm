---
format: aep.planning-md/3
id: approval-record:wave-2026-11-21-empty-signed-reasoning
kind: approval-record
status: accepted
title: 'Wave 2026-11-21-empty-signed-reasoning: signed reasoning with no text streams as a thinking block'
relations:
- decides: story:empty-signed-reasoning
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-21T04:34:25Z", actor: "human:timo", revision: 2}
---
## Stop

Wave 2026-11-21-empty-signed-reasoning on `wave/2026-11-21-empty-signed-reasoning`, based on
`origin/main` 9d669a53. One unit, targeted tests, one full gate, one pull request through the bot App.
No merge, tag or release in this approval: the pull request stops open with its checks read.
llm-gateway takes the change once a release carries it.

| unit | story | branch |
| --- | --- | --- |
| U1 | story:empty-signed-reasoning | impl/empty-signed-reasoning |

## Reason

The operator, 2026-11-21: "we need to fix the LLM gateway then. You can just do that in this
session."

Commits this approval covers: the opening `plan:` commit, the unit's `test:`, `fix:` and `docs:`
commits, its merge into the wave branch, the closing `plan:` commit and the bot pull request.
