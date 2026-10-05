---
format: aep.planning-md/3
id: credential-blocker:anthropic-live-qualification
kind: credential-blocker
status: open
title: The live API and subscription runs need the operator's credentials
relations:
- blocks: story:anthropic-access
revision: 3
---
## What is missing

A successful live subscription turn and credential-rotation report are still missing for
story:anthropic-access. The API-key route remains deferred in
story:anthropic-api-qualification at the operator's request.

The operator authorized and completed subscription-token setup on 2026-10-05. The token is
stored in the platform keychain through b10x/secrets; no additional credential acquisition is
needed for the next turn. The first live request was accepted, but llm refused an undeclared
Messages response field. Wave 2026-10-05-w53 adds field-path diagnostics and response-only capture.

On resuming Claude session 457b5cc2-de5e-4a74-ad2a-227828e4945b, the diagnostic example was run
at 2026-10-05T20:42:02Z. The endpoint returned HTTP 429, dispatch rejected, with Retry-After
8877 seconds. No Messages body was supplied. Do not retry before approximately
2026-10-05T23:10:00Z (2026-10-06 01:10 Europe/Berlin); expiry does not guarantee admission.
The private local report is `~/.cache/ga-wave-2026-10-05-w53/resume-live-report-1.json` and its
response-header capture is `resume-response-1.sse` beside it. Neither is a successful
qualification or evidence identifying the original refused field.

The 429 was the subscription's own five-hour limit (`anthropic-ratelimit-unified-representative-claim:
five_hour`), shared with the operator's other Claude use. Rerun at 2026-10-05T23:46Z: HTTP 200, refused
at `message_start.message.container`; `message_start.message` also carries `stop_details` and
`diagnostics` (all null in that turn). Capture: `~/.cache/ga-wave-2026-10-05-w53/live-response-2.sse`
(response only, mode 0600). The diagnostics work is story:live-diagnostics.

## Clears when

After the provider's retry interval, run the diagnostic example once with a new capture path.
Use the actual refused field and response to add a deterministic regression test, then fix the
decoder under the existing supported-subset contract. Record a successful live turn and the
credential-rotation report required by the story. Keep the story active until that evidence exists.

## Also recorded here

Adversary finding F4 (review-result:adversary-w48-llm-anthropic-access-pass-1): nothing in llm keeps
a subscription-oauth binding to its operator. beyond10x/llm-gateway must not serve a subscription
route to other callers; the quoted terms forbid it.
