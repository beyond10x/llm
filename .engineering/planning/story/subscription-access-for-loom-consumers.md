---
format: aep.planning-md/3
id: story:subscription-access-for-loom-consumers
kind: story
status: draft
title: A Loom consumer can rely on qualified Anthropic and OpenAI subscription access
summary: 'Issue 13: held until the operator names the subscription accounts'
tags:
- from-issue
refs:
- provider: github
  reference: beyond10x/llm#13
relations:
- decomposes: epic:github-zero-2026-11-09
- depends_on: story:anthropic-access
- depends_on: story:openai-access
- serves: vision:portable-model-inference
revision: 1
---
## Context

GitHub issue 13, <https://github.com/beyond10x/llm/issues/13>, read 2026-11-09. A downstream
consumer wants to run Loom on subscription credentials (Claude and Codex logins) and asks for two
things:

1. Anthropic and OpenAI subscription access qualified, so a caller can rely on subscription
   presentation through `llm-credentials`.
2. An Anthropic subscription credential source beside `codex-auth-file`, if one is in scope.

## What the repository settles today

Read at `origin/main` `7c56b19b`:

- The Anthropic subscription presentation is built and fixture-checked. A `subscription-oauth`
  account resolves its token through its secret reference, and Messages sends it as a bearer with
  the OAuth beta header and the client preamble:
  `crates/llm-messages/tests/subscription.rs::a_subscription_turn_presents_the_token_as_a_bearer_with_the_oauth_beta_and_the_preamble`,
  `::a_rotated_subscription_token_is_presented_on_the_next_turn`,
  `::the_subscription_request_matches_its_pinned_fixture`. `docs/implementation-status.md:138`
  says the same and that no live turn is recorded.
- Ask 2 needs no new source for the token itself: the token is any secret reference the caller's
  resolver answers (`crates/llm-providers/src/auth.rs:78`). A reader of Claude Code's own login
  file is not in scope: the operator decided on 2026-10-05 that llm reads no token file
  (`approval-record:access-decisions-2026-10-05`, `story:credentials-from-secrets`), and login
  flows are not owned here (AGENTS.md, "Owns, and does not own").
- A routes document cannot declare a subscription login
  (`crates/llm-models/src/routes.rs:638-640`); a caller builds the account through the catalog.

## What nothing in the repository can settle

Ask 1 is a live qualification. `approval-record:subscription-access-2026-10-23` (option A of
`decision-blocker:subscription-access-contract`) decided that llm uses no subscription credential of
a provider until the operator has recorded the account llm may use and that the terms read allow
it. Neither account is recorded:

- `credential-blocker:anthropic-live-qualification` blocks `story:anthropic-access`.
- `credential-blocker:openai-subscription-account` blocks `story:openai-access`.

Both now also block this story. The evidence read for that decision is
`verification-report:subscription-access-evidence-2026-10-23`.

## Outcome

A Loom consumer can rely on subscription presentation for Anthropic and OpenAI because each route
has a recorded live qualification on an account the operator named.

## Acceptance

`story:anthropic-access` and `story:openai-access` are implemented on their own acceptance (a
recorded successful live subscription turn and rotation report per provider), and
`docs/implementation-status.md` lists both as qualified. Issue 13 is closed with those two reports.
Until then the issue stays open.
