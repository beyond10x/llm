---
format: aep.planning-md/3
id: review-result:adversary-w48-llm-anthropic-access-pass-1
kind: review-result
status: active
title: Wave 2026-10-05-w48 adversary, llm story:anthropic-access, pass 1
relations:
- reviews: story:anthropic-access
revision: 1
---
```
unit: beyond10x/llm story:anthropic-access, branch impl/anthropic-access at 077d78a9 plus 2 untracked test files
verdict: CONFIRMED (no red case; 3 judgement findings, 1 note)
cases: executed 117→123, red 0
origin: introduced 3 / pre-existing 0 / undecided 1
wrote-outside-worktree: 1 (build dir, deleted)
needs-coordinator: none
```

Cases added (all green): a redirect to another host never receives the subscription token; a retry
after a refused subscription turn presents the rotated token with the beta header once; the
preamble opens `system` for seven instruction shapes and only under a subscription; a bearer billed
as subscription sends neither the beta header nor the preamble; only subscription billing over
Messages binds in any declaration order; the token reaches no diagnostic.

Held: redirects (no redirect policy), proxies (no_proxy), error bodies, Debug output, malformed-token
refusals; billing and protocol refusal in every combination and through the catalog; the beta
header exactly once on subscription turns; preamble placement next to cache breakpoints; token
rotation between turns and before a retry; every parity citation for C8, C21, M5, M6, M44.

```findings
[
  {"file":"docs/messages.md","line":42,"category":"contract-drift","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"the Messages projection doc still says system is one block and absent for an empty instruction, false for every subscription-oauth request"},
  {"file":"website/docs/concepts/credentials.md","line":53,"category":"judgement","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"never billed as metered holds per account only; the catalog accepts a bearer metered account sharing the subscription account's secret_reference_id, which sends the same token off Messages under metered billing"},
  {"file":"crates/llm-routing/src/fallback.rs","line":230,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"a 429 from a subscription target counts as eligible for fallback, so a mixed route moves the turn to a metered target, against the story's no API-billing substitution"},
  {"file":".engineering/planning/approval-record/access-decisions-2026-10-05.md","category":"judgement","severity":"note","verdict":"INFEASIBLE","origin":"undecided","message":"nothing in llm keeps a subscription-oauth binding to its operator; llm-gateway in another repository could serve it to other callers"}
]
```
