---
format: aep.planning-md/3
id: story:credentials-from-secrets
kind: story
status: draft
title: Credentials come only from the secrets library; llm reads no token file
relations:
- decomposes: epic:access
- depends_on: story:secrets-resolver
- serves: vision:portable-model-inference
revision: 1
---
## Outcome

llm reads no credential from a file of its own choosing: every `SecretRef` resolves through the
secrets library (`story:secrets-resolver`). The file-reading adapters in `llm-credentials`
(`FileResolver`, `CodexAuthFile` and the JSON-pointer resolver over a file) are retired or moved
behind the secrets library.

## Why

Operator, 2026-10-05: "we dont want to have a file with tokens at all, stuff must come dfrom
b10x/secrets" (approval-record:access-decisions-2026-10-05).

## Open

- UNMAPPED: the Codex login lives in Codex's own `auth.json` and is renewed in place
  (`story:parity-codex-renewal`). Whether the secrets library takes custody of it, reads it, or
  the Codex route moves elsewhere is not decided; ask the operator before this story is scoped.
- UNMAPPED: Harness and Loom consumers that configure a file source today.

## Acceptance

`cargo tree -e features` shows no file-reading credential feature enabled by any consumer route;
the secrets-library route resolves every credential the qualification uses.
