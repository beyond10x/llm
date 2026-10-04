---
format: aep.planning-md/3
id: story:parity-codex-renewal
kind: story
status: draft
title: llm renews a Codex login and writes it back atomically
relations:
- decomposes: epic:serving-split
- informed_by: story:harness-parity
revision: 1
---
## Outcome

llm renews a Codex login and writes it back atomically.

## Rows

From `docs/harness-parity.md` (`story:harness-parity`, wave 2026-10-05-w20), Gaps lines verbatim:

- C14: llm refuses to overwrite a credential file that changed during renewal
- C18: llm decides what to do with an access token whose expiry cannot be read
- C19: llm renews or refuses a token inside a stated margin before it expires
- C20: llm completes the Codex login source with renewal
- C9, C10, C11, C12, C13, C15, C16, C22: llm renews a Codex login through its token endpoint and writes it back atomically, byte-preserving
- H33: llm-http performs a single non-retried JSON POST for a credential exchange

## Acceptance

Each row above becomes `covered` in `docs/harness-parity.md`: llm has the behaviour and a named
test exercises it, cited at `path:line`; or the row says why llm deliberately differs.
