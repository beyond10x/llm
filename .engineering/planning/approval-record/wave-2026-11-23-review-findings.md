---
format: aep.planning-md/3
id: approval-record:wave-2026-11-23-review-findings
kind: approval-record
status: accepted
title: 'Wave 2026-11-23-review-findings: review findings 1, 2 and 9 of llm 0.24.0'
summary: Three implemented units merged into one wave branch, gated once, opened as one bot pull request
relations:
- decides: story:final-only-answer-text
- decides: story:attributed-cost
- decides: story:usage-reader
revision: 2
transitions:
- {from: "draft", to: "accepted", at: "2026-11-23T02:24:23Z", actor: "human:timo", revision: 2, executor: "agent:llm-wave-review-findings", correlation: "2026-11-23-review-findings"}
---
## Stop

Wave 2026-11-23-review-findings on `wave/2026-11-23-review-findings`, based on `origin/main`
08289a72 (0.24.0). Three units, each already implemented and tested on its own branch, merged with
`--no-ff`, one gate on the merged tree, one pull request through the bot App. No merge to `main`,
tag or release in this approval: the pull request stops open with its checks started. llm-gateway
takes `story:usage-reader` once a release carries it (its `dependency-blocker:llm-usage-decoders`).

| unit | story | branch | review finding |
| --- | --- | --- | --- |
| U1 | story:final-only-answer-text | impl/final-only-answer-text | 2 |
| U2 | story:attributed-cost | impl/attributed-cost | 9 |
| U3 | story:usage-reader | impl/usage-reader | 1 |

Not in this wave: finding 3 (the Responses relay pairs streamed and final messages by position) and
finding 8 (Responses drops a reported cache-write count).

## Reason

The operator, 2026-11-23, on the review of llm 0.24.0 (`08289a72`): "address those", naming three
findings: the `llm` command printed nothing when the answer text arrived only in the final object
(finding 2); a turn was priced the same whatever model the provider reported (finding 9); LLM
Gateway's usage records lacked the reported model and the reasoning-token count (finding 1).

Consequence named for review: with `story:attributed-cost` a Bedrock Converse route's cost is always
unknown, because a Converse answer reports no model.

Commits this approval covers: the opening `plan:` commit, each unit's `test:`, `fix:`, `feat:` and
`plan:` commits already on its branch, the three merges into the wave branch, any merge-fallout fix,
the closing `plan:` commit and the bot pull request.
