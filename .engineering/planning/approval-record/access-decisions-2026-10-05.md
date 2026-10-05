---
format: aep.planning-md/3
id: approval-record:access-decisions-2026-10-05
kind: approval-record
status: draft
title: 'Operator decisions 2026-10-05: build Anthropic subscription access; no token files'
relations:
- decides: story:anthropic-access
revision: 1
---
## Decision

Operator, 2026-10-05, asked "Anthropic subscription access in llm: which way?" with options A (API
key only), B (ask Anthropic first) and C (build it anyway): **C**.

llm builds Anthropic subscription access (parity rows C8, C21, M5, M6, M44): a caller-supplied
subscription OAuth token presented on Messages with the OAuth beta header and the subscription
client preamble. llm owns no login; the token comes from the secrets library (second decision).

## Terms read before the decision

https://code.claude.com/docs/en/legal-and-compliance, "Authentication and credential use", read
2026-10-05:

> Anthropic does not permit third-party developers to offer Claude.ai login into their own
> applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their
> users. Moreover, developers may not collect, store, or intermediate Claude.ai credentials or
> session tokens

> OAuth authentication is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and
> Enterprise subscription plans and is designed to support ordinary use of Claude Code and other
> native Anthropic applications.

> Anthropic reserves the right to take measures to enforce these restrictions and may do so without
> prior notice.

The operator was shown these and chose C with the account risk named. The route is for the
operator's own subscription; llm offers no login and serves no other user's credentials.

## Second decision: no token files

Operator, 2026-10-05, on parity row C1 (a token file with leading whitespace): "what the heck is a
token file? we dont want to have a file with tokens at all, stuff must come dfrom b10x/secrets".
Row C1 is not needed; credentials come from the secrets library (`story:secrets-resolver`), and
`story:credentials-from-secrets` retires the file-reading adapters.
