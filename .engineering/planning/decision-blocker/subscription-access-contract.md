---
format: aep.planning-md/3
id: decision-blocker:subscription-access-contract
kind: decision-blocker
status: cleared
title: Direct subscription integration needs provider-specific access evidence
relations:
- blocks: story:openai-access
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-23T22:01:04Z", actor: "human:timo", revision: 3}
---
## Missing evidence

The supported direct subscription contract and permitted deployment context for each provider must be established before claiming qualification. Technical access to a token is not that evidence. Caller-owned credentials/refresh are fixed decisions; LLM will not own login.

Anthropic currently describes third-party credential handling restrictions at https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use (read 2026-09-19). OpenAI documents subscription authentication for Codex at https://learn.chatgpt.com/docs/auth, not a universal replacement for API billing.

## Clears when

Record provider-specific documentation/account authorization and tested endpoint/auth behavior supporting the selected direct embedding/gateway use. Do not silently remove requested subscription coverage or weaken foundation-qualified. API work and emulated protocol work can continue.

## Resolution

Cleared 2026-10-23 by approval-record:subscription-access-2026-10-23. The owner answered "5A" on
2026-10-23 (relayed by `b10x-chef-orchestrator`): option A, llm qualifies direct subscription use
only for providers whose accounts and terms the operator records.

The per-provider evidence this blocker asked for (documents read with their dates, what the terms
allow for third-party or embedded use, tested endpoint and auth behaviour) is
verification-report:subscription-access-evidence-2026-10-23. What remains after the decision is the
account each route runs on, which the operator names; that is now one credential blocker per
provider: credential-blocker:openai-subscription-account and
credential-blocker:anthropic-live-qualification. No subscription credential is used until the
operator names the account.
