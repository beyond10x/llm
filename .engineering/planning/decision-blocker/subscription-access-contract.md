---
format: aep.planning-md/3
id: decision-blocker:subscription-access-contract
kind: decision-blocker
status: open
title: Direct subscription integration needs provider-specific access evidence
relations:
- blocks: story:openai-access
- blocks: story:anthropic-access
revision: 1
---
## Missing evidence

The supported direct subscription contract and permitted deployment context for each provider must be established before claiming qualification. Technical access to a token is not that evidence. Caller-owned credentials/refresh are fixed decisions; LLM will not own login.

Anthropic currently describes third-party credential handling restrictions at https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use (read 2026-09-19). OpenAI documents subscription authentication for Codex at https://learn.chatgpt.com/docs/auth, not a universal replacement for API billing.

## Clears when

Record provider-specific documentation/account authorization and tested endpoint/auth behavior supporting the selected direct embedding/gateway use. Do not silently remove requested subscription coverage or weaken foundation-qualified. API work and emulated protocol work can continue.
