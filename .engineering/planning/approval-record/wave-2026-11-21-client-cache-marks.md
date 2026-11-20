---
format: aep.planning-md/3
id: approval-record:wave-2026-11-21-client-cache-marks
kind: approval-record
status: accepted
title: 'Wave 2026-11-21-client-cache-marks: a client''s cache marks reach Messages and InvokeModel as placed'
relations:
- decides: story:client-cache-marks
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-20T23:53:17Z", actor: "agent:ai-agent-platform", revision: 2}
---
## Stop

Wave 2026-11-21-client-cache-marks on `wave/2026-11-21-client-cache-marks`, based on `origin/main`
b140e094. One unit, targeted tests, one full gate, one pull request through the bot App. No merge,
tag or release in this approval: the pull request stops open with its checks read. llm-gateway
takes the change once a release carries it (its `story:bedrock-client-cache-marks`).

| unit | story | branch |
| --- | --- | --- |
| U1 | story:client-cache-marks | impl/client-cache-marks |

## Reason

The operator, 2026-11-21: "you do need to make sure that we're not dropping any behavior. So if
LLM gateway is having that weird behavior of dropping the effort, make sure this isn't happening.
So we need to fix the LLM gateway then. You can just do that in this session."

Commits this approval covers: the opening `plan:` commit, the unit's `test:`, `feat:`, `spec:`
and `docs:` commits, its merge into the wave branch, the closing `plan:` commit and the bot pull
request.
