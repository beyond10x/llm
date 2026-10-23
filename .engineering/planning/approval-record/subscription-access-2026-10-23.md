---
format: aep.planning-md/3
id: approval-record:subscription-access-2026-10-23
kind: approval-record
status: draft
title: Direct subscription use only for providers whose accounts and terms the operator records, decided by the owner on 2026-10-23
summary: 'Option A (5A): llm qualifies a direct subscription route only for a provider whose account and terms the operator has recorded; no subscription credential is used until the operator names the account'
relations:
- decides: decision-blocker:subscription-access-contract
- decides: story:openai-access
- decides: story:anthropic-access
revision: 1
---
## The stop

decision-blocker:subscription-access-contract: which direct subscription routes llm may qualify,
and on what evidence.

## What authorises passing it

The owner, 2026-10-23, answered the blocker with "5A", relayed by session `b10x-chef-orchestrator`
with the option's text: "Option A: llm qualifies direct subscription use only for providers whose
accounts and terms the operator records."

## Decision

- llm qualifies a direct subscription route (a caller-supplied subscription token that llm presents
  to the provider's own endpoint) only for a provider whose account and terms the operator has
  recorded in this store: the account llm may use, the documents read, and what they allow.
- Until the operator names the account for a provider, llm uses no subscription credential of that
  provider: no live turn, no capture, no renewal request. The two captures of 2026-10-21 stay as the
  recorded fixtures they are.
- API-key routes and emulated work (fixtures, loopback doubles, the conformance suite) continue
  unchanged.
- verification-report:subscription-access-evidence-2026-10-23 holds the per-provider evidence and
  names the accounts the operator must confirm. credential-blocker:openai-subscription-account and
  credential-blocker:anthropic-live-qualification keep story:openai-access and
  story:anthropic-access blocked until the operator has named them.

This clears decision-blocker:subscription-access-contract.
