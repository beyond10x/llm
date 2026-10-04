---
format: aep.planning-md/3
id: story:codex-auth-file
kind: story
status: implemented
title: A Codex login resolves to its access token, read-only
relations:
- decomposes: epic:access
- serves: vision:portable-model-inference
scope:
- confidence: cited
  path: README.md
- confidence: cited
  path: contracts/
- confidence: cited
  path: crates/llm-credentials/Cargo.toml
- confidence: cited
  path: crates/llm-credentials/src/codex.rs
- confidence: cited
  path: crates/llm-credentials/src/lib.rs
- confidence: cited
  path: crates/llm-credentials/tests/codex.rs
- confidence: cited
  path: docs/
- confidence: cited
  path: spec/domains/catalog.yaml
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-04T14:12:24Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-04T14:12:24Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-04T15:09:19Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":4,"verification":1}}}
---
## Outcome

`llm-credentials` gains a read-only resolver for a Codex login, behind a feature
(`codex-auth-file`, off by default, like `file`).

- `CodexAuthFile::new(reference, path)` resolves one `SecretRef` to the `/tokens/access_token` of
  the `auth.json` at the given absolute path, reading it on every request. There is no default
  path: the crate does no ambient lookup, so the embedding application expands
  `~/.codex/auth.json` (intake `story:model-access`). Decided in wave 2026-10-04-w16, adversary
  pass 1.
- It never writes the file and never refreshes the token. Renewal is the caller's, and a Codex login
  is renewed by running `codex` (story `openai-access` context: never rewrite a vendor-owned auth
  document).
- An expired or missing token, judged by the JWT `exp` claim against the caller's clock, is refused
  as `Expired` or `Missing`. The message names the file and says to run `codex` to refresh the
  login.
- The token never enters a `Debug` or `Display`.

## Acceptance

`a_codex_login_resolves_its_access_token`, against fixture files:
- a valid file resolves the token;
- an expired token is refused as `Expired`, naming the file;
- a missing file or a missing pointer is refused as `Missing`;
- the fixture file's bytes and mode are unchanged after every call;
- `format!("{:?}", resolver)` contains no token byte.

## ESS first

If `spec/` declares credential-source kinds, the Codex auth file lands there first and validates;
otherwise none, and the first commit is the named test, red because the resolver does not exist.

## Live qualification

One successful turn on `https://chatgpt.com/backend-api/codex` with this resolver and
`ResponsesClient` (model `gpt-5.6-sol`), run by the operator's slice (beyond10x/intake), is the
provider-specific access evidence `decision-blocker:subscription-access-contract` asks for. It is
recorded on `story:openai-access`. Harness recorded the same route working on 2026-08-30
(harness `story:chatgpt-codex-authorized-run`).
