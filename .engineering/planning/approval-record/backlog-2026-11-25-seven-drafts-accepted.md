---
format: aep.planning-md/3
id: approval-record:backlog-2026-11-25-seven-drafts-accepted
kind: approval-record
status: accepted
title: Seven drafts are accepted and form the wave plan after wave 8
relations:
- decides: story:test-binaries-one-per-crate
- decides: story:llm-decide-tests-read-no-clock-and-build-one-binary
- decides: story:messages-cancellation-case-waits-on-a-signal
- decides: story:tool-arguments-with-a-repeated-key
- decides: story:replay-stops-at-the-last-case-have-scenarios
- decides: story:responses-carries-refusal-text
- decides: story:cli-route-explain
revision: 4
transitions:
- {from: "draft", to: "accepted", at: "2026-11-25T14:06:38Z", actor: "human:operator", revision: 3}
---
## Decision

Seven drafts are accepted and form the wave plan after wave 8, in this order: test defects first,
correctness second, features last.

Test defects:

- `story:test-binaries-one-per-crate`
- `story:llm-decide-tests-read-no-clock-and-build-one-binary`
- `story:messages-cancellation-case-waits-on-a-signal`

Correctness:

- `story:tool-arguments-with-a-repeated-key`
- `story:replay-stops-at-the-last-case-have-scenarios`

Features:

- `story:responses-carries-refusal-text`
- `story:cli-route-explain`

## Source

The coordinator put the question on 2026-11-25, when wave 8 had merged and the only stories left
`active` were wave 7's (waiting for a `ux-browser` release) and `story:anthropic-access` (held by
its credential blocker). Four options: A, the three test-rule drafts; B, A and the two correctness
drafts; C, B and the two features; D, none. The operator session that acts with the owner's
permissions answered on 2026-11-25: "D34 = C, accepted by me on [the owner's] delegation: the 3
test-rule drafts, the 2 correctness drafts, then the 2 features
(`story:responses-carries-refusal-text`, `story:cli-route-explain`). Order: test defects first,
correctness second, features last, as the org rule says. Reason: none needs a credential, a live
call or money, and no accepted story is left."

## What it changes

Each of the seven moves `draft` to `proposed` to `active`, gets its Scope section from one
`story-scoper`, and enters the wave plan `aep plan artifact waves` derives from the active
stories. `story:llm-calls-builds-one-integration-test-binary` stays a draft: the workspace story
`story:test-binaries-one-per-crate` covers its crate. No story joins wave 7.
