---
format: aep.planning-md/3
id: credential-blocker:messages-cache-capture
kind: credential-blocker
status: cleared
title: The live Messages cache capture needs subscription quota or another account's token
relations:
- blocks: story:live-provider-captures
withholds: test_result
revision: 2
transitions:
- {from: "open", to: "cleared", at: "2026-10-21T09:51:40Z", actor: "human:timo", revision: 2}
---
## What is missing

The Messages half of story:live-provider-captures: one live Anthropic turn pair with its cache
write and cache read figures, recorded through the subscription token `llm` already holds.

Two attempts on 2026-10-21 through `live_subscription_turn` returned HTTP 429. The response headers
said the account's 7-day window was used up: `anthropic-ratelimit-unified-7d-status: rejected`,
utilization 1.0, overage `out_of_credits`, `retry-after: 262858`, reset at Unix 1792836000
(2026-10-24 10:00 UTC). The captures are private, mode 0600, under `~/.cache/llm-captures/raw/`.

The operator's approval (approval-record:live-provider-captures-2026-10-21) covers no new
credential. A token for another account is a new credential.

## Clears when

Either the 7-day window resets (not before 2026-10-24 10:00 UTC; a reset does not guarantee
admission), or the operator stores a subscription token for another account under the same
secret name. Then record the two turns, scrub them, and add the fixtures and replay test the story
names.
