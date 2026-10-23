---
format: aep.planning-md/3
id: credential-blocker:openai-subscription-account
kind: credential-blocker
status: open
title: The OpenAI subscription route needs the operator to name the ChatGPT account and route
relations:
- blocks: story:openai-access
revision: 1
---
## What is missing

The ChatGPT account llm may use for the subscription route, named by the operator, as option A of
decision-blocker:subscription-access-contract requires
(approval-record:subscription-access-2026-10-23).

The route llm has tested is the Codex backend, `https://chatgpt.com/backend-api/codex`, with the
Codex login at `~/.codex/auth.json`. The route OpenAI documents for a third-party open-source client
that spends a user's ChatGPT plan is Sign in with ChatGPT on `https://api.openai.com/v1/responses`,
with a registration the client runs itself; llm owns no login, so a caller would run it and hand
the token to llm through its secret resolver. No OpenAI document read on 2026-10-23 permits or
forbids the first route for a client other than Codex; OpenAI's Terms of Use could not be read
(HTTP 403). Both are in verification-report:subscription-access-evidence-2026-10-23.

Until this clears, llm makes no request with a Codex login or any other ChatGPT subscription
credential: no live turn, no capture, no renewal request. The capture of 2026-10-21 stays a fixture.

## Clears when

The operator records, here or in an approval record: the ChatGPT account and plan llm may use; that
the terms the operator has read allow that account on the named route (the Codex backend, Sign in
with ChatGPT, or neither); and the token source llm reads it from. Then story:openai-access can run
its live subscription report on that account and route.
