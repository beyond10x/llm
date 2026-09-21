---
title: Credentials
description: An opaque reference, resolution at request time, coordinated renewal, and optional local adapters that read but never write.
---

# Credentials

Inference accepts an injected `SecretResolver`. A `SecretRef` is a validated opaque lookup name —
not a secret value, and not a choice of storage backend.

## The guarantees

- Resolution happens **at request time**, so rotation works without restarting the caller.
- Returned material is zeroized on drop, redacted in `Debug`, and has no `Serialize` or `Display`
  implementation.
- LLM never searches ambient vendor directories, runs a login flow or writes a credential file.
- Configuration carries references, never values. An entry value never belongs in argv, TOML,
  tracing or a diagnostic.
- A coordinated resolver serializes resolution and refresh per reference, and refreshes only the
  credential generation that was actually rejected — so concurrent callers cannot refresh the same
  generation repeatedly.

## Anonymous is a first-class mode

Anonymous accounts require an **absent** reference; authenticated modes require a **present** one.
Billing kind — metered, subscription or self-hosted — is independent of both protocol and
authentication presentation. A rejected subscription credential never changes the billing kind or
the account.

## Optional local adapters

The default feature set has no file or native credential-store dependency. The adapters are opt-in:

| Feature | Source | Availability |
| --- | --- | --- |
| `file` | An explicit absolute path per reference | Linux; other platforms return `UnsupportedPlatform` on resolve |
| `keychain` | An explicitly injected `keyring_core::CredentialStore` with a service and entry per reference | Any compatible injected store |
| `native-keychain` | A native-store constructor; includes `keychain` | Linux Secret Service, macOS Keychain, Windows Credential Manager |

These adapters **read existing material only**. They do not create entries, perform a login,
refresh a token, search a vendor configuration directory, pick another source when a reference is
absent, or modify the process-global keyring store. Provisioning and rotation are the operator's.

They identify exact content rather than issuer generations, so they return `RefreshUnsupported`;
independently replaced bytes are visible on the next resolve.

[Resolve a local secret](../guides/resolve-a-local-secret.md) shows the file adapter's protection
rules.
