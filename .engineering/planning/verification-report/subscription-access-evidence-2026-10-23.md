---
format: aep.planning-md/3
id: verification-report:subscription-access-evidence-2026-10-23
kind: verification-report
status: draft
title: 'Per-provider subscription access evidence: documents read 2026-10-23, what the terms allow, tested endpoint and auth behaviour, accounts to confirm'
relations:
- verifies: story:openai-access
- verifies: story:anthropic-access
- informed_by: approval-record:subscription-access-2026-10-23
revision: 2
---
## Scope

Evidence for option A of decision-blocker:subscription-access-contract
(approval-record:subscription-access-2026-10-23): per provider, the documents read, what they say
about a third-party or embedded client presenting a subscription credential, what llm has tested on
that provider, and the account the operator must name before llm uses a subscription credential of
that provider again. Every document was read on 2026-10-23 unless a row says otherwise. No provider
was called for this report. Quotations are verbatim; everything else in a "terms" cell is a reading
and is marked as such.

## Per provider

| Provider | Documentation (read 2026-10-23) | What the terms allow for third-party or embedded use | Tested endpoint and auth behaviour | Account the operator must confirm |
| --- | --- | --- | --- | --- |
| Anthropic, Claude subscription (`subscription-oauth` on Messages) | <https://code.claude.com/docs/en/legal-and-compliance>, "Authentication and credential use"; <https://code.claude.com/docs/en/authentication>, "Generate a long-lived token" and "Credential management"; <https://www.anthropic.com/legal/consumer-terms>, effective 2025-10-08, "Use of our Services". Quotes below. | OAuth "is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and Enterprise subscription plans and is designed to support ordinary use of Claude Code and other native Anthropic applications"; developers "should use API key authentication"; no third-party Claude.ai login, no routing of other users' plan credentials, no collecting, storing or intermediating "Claude.ai credentials or session tokens"; enforcement "without prior notice". `claude setup-token` is documented for "CI pipelines, scripts" and "can only make model requests". Reading: no sentence read permits a client other than Claude Code or a native Anthropic application to present a subscription token, and none names the operator's own token in the operator's own embedding; the case is unaddressed and enforcement is reserved. The 2026-10-05 decision (approval-record:access-decisions-2026-10-05) took that account risk for the operator's own subscription. | `POST https://api.anthropic.com/v1/messages`; `authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20`, `anthropic-version: 2023-06-01` (`crates/llm-messages/src/client.rs:26-33`), `system` opening with the client preamble (`crates/llm-messages/src/codec.rs:13`). 2026-10-05T20:42Z: HTTP 429, the subscription's five-hour limit, Retry-After 8877 s. 2026-10-05T23:46Z: HTTP 200, refused at `message_start.message.container`, fixed by story:live-diagnostics. 2026-10-21: two HTTP 200 turns, `claude-haiku-4-5-20251001`, cache write then cache read of 13027 tokens (`crates/llm-messages/tests/fixtures/live/README.md`). Tokens used: keychain `llm/anthropic-subscription`, minted with `claude setup-token` on 2026-10-05 (credential-blocker:anthropic-live-qualification), and the machine's active Claude login for the 2026-10-21 capture (approval-record:live-capture-claude-login-2026-10-21). Not run: the rotation check (`--rotate-check`), any API-key turn. | The Claude account and plan (Pro, Max, Team or Enterprise) behind keychain secret `llm/anthropic-subscription`, and whether that is the one account llm may use. The 2026-10-21 capture used the machine's active login, which may be another account. |
| OpenAI, ChatGPT subscription (Codex login as a bearer on Responses) | <https://learn.chatgpt.com/docs/auth> (Codex authentication; `https://developers.openai.com/codex/auth` answers 308 to it); <https://learn.chatgpt.com/docs/pricing>; <https://learn.chatgpt.com/docs/sign-in-with-chatgpt>; <https://developers.openai.com/siwc/token-sharing-open-source> with its pages `sign-in`, `models-and-inference`, `token-reference`, `preview-limitations`. OpenAI Terms of Use: not read, see below. Quotes below. | Codex docs: ChatGPT sign-in is for "the ChatGPT desktop app, Codex CLI, and IDE extension"; `~/.codex/auth.json` is to be treated "like a password"; "API keys are still the recommended default for automation"; "For general OpenAI API calls, continue to use Platform API keys." Reading: no sentence read permits or forbids a client other than Codex presenting the Codex login's token to `chatgpt.com/backend-api/codex`. The route OpenAI documents for a third-party "open-source and locally hosted" app that spends a user's ChatGPT plan is Sign in with ChatGPT: the app registers its own OAuth client without a secret, the user grants `chatgpt.tokens.use.direct`, and the app calls `POST https://api.openai.com/v1/responses` with the bearer token, `store: false` and `stream: true`; "do not point it at ChatGPT's `backend-api` endpoints". Eligible users: "ChatGPT Plus and Pro". "If you're interested in offering it in a paid or remotely hosted app, complete the interest form." The registration is a login flow, which llm does not own; a caller would run it and hand the token to llm through its secret resolver. | `POST https://chatgpt.com/backend-api/codex/responses` (`crates/llm-tool-call/src/lib.rs:47`, `crates/llm-responses/src/binding.rs:7`); `authorization: Bearer <access_token>` read from `~/.codex/auth.json` at `/tokens/access_token` (`CodexAuthFile`), plus, when a conversation is set, `originator`, `session-id` and `x-client-request-id` (`crates/llm-responses/src/conversation.rs:56-71`). 2026-10-04 probe: two incompatibilities, fixed in 0.1.6 (`docs/implementation-status.md`). 2026-10-21T08:58Z: HTTP 200, `gpt-6-astra`, 35 events, plan and usage response headers `x-codex-plan-type`, `x-codex-primary-used-percent`, `x-codex-primary-reset-after-seconds`, `x-codex-credits-balance` (`crates/llm-responses/tests/fixtures/live/README.md`). Renewal `POST https://auth.openai.com/oauth/token` (`crates/llm-credentials/src/codex/renewal.rs:59`): loopback tests only, never live. Never tested: `api.openai.com/v1/responses` with a Sign in with ChatGPT token, any API-key turn. | The ChatGPT account and plan behind the Codex login at `~/.codex/auth.json` on the operator's machine, and which route the operator allows it on: the Codex backend, Sign in with ChatGPT, or neither. |

## Anthropic: quoted terms

<https://code.claude.com/docs/en/legal-and-compliance>, section "Authentication and credential use",
read 2026-10-23 (the page shows no date):

> OAuth authentication is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and
> Enterprise subscription plans and is designed to support ordinary use of Claude Code and other
> native Anthropic applications.

> Developers building products or services that interact with Claude's capabilities, including
> those using the Agent SDK, should use API key authentication through Claude Console or a
> supported cloud provider. Anthropic does not permit third-party developers to offer Claude.ai
> login into their own applications, or to route requests through Free, Pro, or Max plan
> credentials on behalf of their users. Moreover, developers may not collect, store, or
> intermediate Claude.ai credentials or session tokens — sign-in to a Claude account must complete
> through Anthropic's own flow.

> Nor does it prevent an end user from signing in to the unmodified Claude Code binary with their
> own Claude subscription

> Anthropic reserves the right to take measures to enforce these restrictions and may do so without
> prior notice.

Same page, section "Acceptable use":

> Advertised usage limits for Pro and Max plans assume ordinary, individual usage of Claude Code and
> the Agent SDK.

<https://code.claude.com/docs/en/authentication>, section "Generate a long-lived token", read
2026-10-23:

> For CI pipelines, scripts, or other environments where interactive browser login isn't available,
> generate a one-year OAuth token with `claude setup-token`

> This token authenticates with your Claude subscription and requires a Pro, Max, Team, or
> Enterprise plan. It can only make model requests

Same page, "Credential management": on Linux the login is stored in `~/.claude/.credentials.json`
with file mode `0600`; `.credentials.json` is managed through `/login` and `/logout`.

<https://www.anthropic.com/legal/consumer-terms>, effective 2025-10-08, section "Use of our
Services", item 7, read 2026-10-23 (fragments as fetched):

> Except when you are accessing our Services via an Anthropic API Key or where we otherwise
> explicitly permit it

> to access the Services through automated or non-human means, whether through a bot, script, or
> otherwise.

Section "Your Anthropic Account":

> You may not share your Account login information, Anthropic API key, or Account credentials with
> anyone else

The Commercial Terms of Service (Team, Enterprise, API) were not read; which terms apply depends on
the plan the operator names.

## OpenAI: quoted terms

<https://learn.chatgpt.com/docs/auth>, read 2026-10-23 (the page shows no date):

> Codex supports two ways for a person to sign in when using OpenAI models:
> Sign in with ChatGPT for subscription access; Sign in with an API key for usage-based access

> The ChatGPT desktop app, Codex CLI, and IDE extension support both sign-in methods for local
> work.

> When you sign in with an API key, Codex uses standard API pricing instead of included ChatGPT plan
> credits.

> Codex caches login details locally in a plaintext file at `~/.codex/auth.json` or in your
> OS-specific credential store.

> For sign in with ChatGPT sessions, Codex refreshes tokens automatically during use before they
> expire

> If you use file-based storage, treat `~/.codex/auth.json` like a password: it contains access
> tokens.

> Access tokens are intended for trusted scripts, schedulers, and private CI runners.

> For general OpenAI API calls, continue to use Platform API keys.

> API keys are still the recommended default for automation.

<https://learn.chatgpt.com/docs/pricing>, read 2026-10-23:

> API token prices are separate from subscription usage; don't use them to estimate included tasks.

> Pro plans currently have no five-hour limit.

> Weekly limits may also apply.

> All users may also run extra local chats using an API key, with usage charged at standard API
> rates.

<https://learn.chatgpt.com/docs/sign-in-with-chatgpt>, read 2026-10-23:

> In supported apps, eligible ChatGPT Plus and Pro subscribers can also choose to use their ChatGPT
> plan for AI requests.

> Allow this app to consume usage from your ChatGPT Plan's limits.

> App usage counts toward your existing plan limits.

<https://developers.openai.com/siwc/token-sharing-open-source> and its pages, read 2026-10-23:

> ChatGPT plan usage is an optional capability within Sign in with ChatGPT.

> These docs explain ChatGPT plan usage for open-source and locally hosted apps.

> If you're interested in offering it in a paid or remotely hosted app, complete the interest form.

> ChatGPT plan usage is available to all open-source partners and selected private clients.

> Sign in with ChatGPT is currently available to selected commercial partners through a limited
> trial.

Page `sign-in`: authorization at `https://auth.openai.com/api/accounts/authorize` with
`client_id=dynamic_agent_client` on first registration, PKCE and a loopback callback on
`127.0.0.1`; token exchange at `https://auth.openai.com/api/accounts/oauth/token`, "No client
secret is required."; scopes `openid profile email` and `offline_access resource.invoke
chatgpt.tokens.use.direct`; "A valid ID token alone does not authorize ChatGPT plan usage."

Page `models-and-inference`:

> To perform inference on behalf of the user, send the OAuth access token as `Authorization: Bearer
> <ACCESS_TOKEN>` to `POST https://api.openai.com/v1/responses`.

> Supported endpoint: Use the public Responses API endpoint above for this ChatGPT plan usage flow;
> do not point it at ChatGPT's `backend-api` endpoints.

> Set `store` to `false` and `stream` to `true` on each HTTP inference request in this flow.

> A usage-limit error can arrive as `response.failed` with `subscription_sharing_usage_limit_exceeded`
> or `subscription_sharing_usage_unavailable` after streaming has begun.

Page `token-reference`: "Access tokens are valid for one hour (`expires_in: 3600`)."; "Refresh
tokens are valid for 30 days."; "Each successful refresh returns a replacement refresh token with a
fresh 30-day lifetime."; audience `https://api.openai.com/v1`, issuer `https://auth.openai.com`.

Page `preview-limitations`: "These limits apply to ChatGPT plan usage through Sign in with ChatGPT
and `https://api.openai.com/v1`, both directly and through Codex app-server."; omit
`previous_response_id` over HTTP; unsupported fields include `background`, `conversation`,
`max_output_tokens`, `metadata`, `prompt_cache_retention`, `safety_identifier`, `temperature`,
`top_p`, `truncation` and `user`; "Your app must refresh the OAuth token."

## Not read

- OpenAI Terms of Use, <https://openai.com/policies/terms-of-use/>: HTTP 403 to two fetches on
  2026-10-23 (one through the harness fetch tool, one with `curl`). The OpenAI row above does not
  cover it; the operator reads it before naming the OpenAI account.
- Anthropic Commercial Terms of Service: not read; see the Anthropic section.
- OpenAI help-centre article "Codex in ChatGPT plans": HTTP 403 on 2026-10-23.

## Accounts the operator must confirm

1. Anthropic: the Claude account and plan behind keychain secret `llm/anthropic-subscription`,
   named as the one account llm may use for the Messages subscription route.
2. OpenAI: the ChatGPT account and plan behind the Codex login at `~/.codex/auth.json`, and the
   route it is allowed on (the Codex backend as tested, Sign in with ChatGPT, or neither).

Until each is confirmed, llm uses no subscription credential of that provider
(approval-record:subscription-access-2026-10-23). API-key routes and emulated work continue.
