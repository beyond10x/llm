---
format: aep.planning-md/3
id: review-result:adversary-w16-llm-responses-client-pass-2
kind: review-result
status: active
title: Wave 2026-10-04-w16 adversary, llm story:responses-client, pass 2
relations:
- reviews: story:responses-client
revision: 1
---
## Adversary pass 2 — llm story:responses-client

Tree: 4a4dcc61 + phase 2 + pass-1 fixes. Verdict NEEDS-CHANGE; cases 90→99, red 3.
New file: `crates/llm-responses/tests/adversary2_client.rs` (9 cases).

| # | where | verdict | finding | decision | outcome |
|---|---|---|---|---|---|
| G1 | `crates/llm-responses/src/client.rs:106` | NEEDS-CHANGE | a sink refusal or cancel during event hand-over returns an error carrying only the binding, dropping the final usage the decoded stream reported | fix: attach the decoded evidence (outcome observation, else the decoder error's, else the binding) | fixed |
| G2 | `crates/llm-responses/src/client.rs:108` | CONFIRMED | decoder refusals returned without attach, so a contradictory-usage refusal after dispatch names no binding | fix: attach on every decoder refusal | fixed |

Mutants: body bound `>` to `>=` (killed by the exact-bound case); outcome refusal with the bare
binding (killed by both outcome cases).

After the fixes: `b10x-llm-responses` 99 passed, 0 failed; workspace 578 passed, 0 failed on the
second run. The first workspace run failed once in `b10x-llm-http`
`headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request` (50 ms header limit,
`transport.rs:16`); that crate is unchanged in this wave, and it passed 5 of 5 isolated runs.
