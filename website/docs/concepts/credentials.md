---
title: Credentials
sidebar_position: 4
description: An opaque reference, resolution at request time, coordinated renewal, and optional adapters for files, keychains, environment variables, JSON documents and a Codex login.
lede: A catalog names a reference, the embedding injects the resolver, and every source llm can read is an opt-in feature that reads only what the caller named.
source: crates/llm-credentials (lib.rs, file.rs, keychain.rs, environment.rs, pointer.rs, codex.rs, codex/renewal.rs, secrets.rs), CHANGELOG.md 0.1.5–0.1.7
---

# Credentials

**Custody is injected.** llm does not own where secrets live. The application that embeds it hands
in a `SecretResolver`, and a catalog names only a `SecretRef`: a validated opaque lookup name, not
a secret value and not a choice of storage backend. The same catalog works unchanged whether the
resolver reads a file, a keychain, an environment variable or something the application wrote
itself.

```rust
pub trait SecretResolver: Send + Sync {
    fn resolve<'a>(&'a self, reference: &'a SecretRef)
        -> BoxFuture<'a, Result<ResolvedSecret, SecretError>>;
    // `refresh` has a default that returns `RefreshUnsupported`.
}
```

## The guarantees

- Resolution happens **at request time**, so rotation works without restarting the caller.
- Returned material is zeroized on drop, redacted in `Debug`, and has no `Serialize` or `Display`
  implementation.
- llm never runs a login flow and looks up nothing the caller did not name: every path, variable
  and pointer is bound to one reference.
- Configuration carries references, never values. A value never belongs in argv, TOML, tracing or
  a diagnostic, and a refusal names the reference, never its path, variable or value.
- A coordinated resolver serializes resolution and refresh per reference, and refreshes only the
  credential generation that was actually rejected, so concurrent callers cannot refresh the same
  generation repeatedly.

## Anonymous is a first-class mode

Anonymous accounts require an **absent** reference; authenticated modes require a **present** one.
Billing kind (metered, subscription or self-hosted) is independent of both protocol and
authentication presentation. A rejected subscription credential never changes the billing kind or
the account.

## Optional adapters

The default feature set has no adapter at all. Each source is a feature of `b10x-llm-credentials`:

| Feature | Resolver | Reads |
| --- | --- | --- |
| `file` | `file::FileResolver` | An explicit absolute path per reference, on Linux, with strict ownership and mode checks |
| `keychain` | `keychain::KeychainResolver` | An explicitly injected `keyring_core::CredentialStore`, one service and entry per reference |
| `native-keychain` | `keychain::native_store()` | Linux Secret Service, macOS Keychain or Windows Credential Manager; includes `keychain` |
| `environment` | `environment::EnvironmentResolver` | One caller-named environment variable per reference, as raw bytes, on every resolve |
| `json-pointer` | `pointer::JsonPointerResolver` | The JSON string at an RFC 6901 pointer in a document another resolver returns |
| `codex-auth-file` | `codex::CodexAuthFile` | The access token of a Codex login's `auth.json` at an explicit absolute path |
| `codex-renewal` | `CodexAuthFile::renewing` | The same, renewing the login when it is due; includes `codex-auth-file` |
| `secrets` | `secrets::SecretsResolver` | The reference as a name in one configured scope of the `secrets` library's storage (v0.5.0); `SecretsResolver::keychain` is its keychain backend behind its local authorizer |

All but `codex-renewal` are **read-only**: they never create an entry, refresh a token or write a
file, so they return `RefreshUnsupported`, and independently replaced bytes are visible on the next
resolve. A document that is not JSON, or a pointer target that is not a string, is
`SecretError::Malformed`: a configuration error, refused like a missing credential.

`secrets` is read-only too, but its `refresh` re-reads rather than returning `RefreshUnsupported`:
the backend's version is the credential's version, so every write is a new one, and
`refresh` is `RefreshRejected` while it is unchanged. A missing name is `Missing`, a
scope the authorizer denies `UnsafeSource`, a backend without read `UnsupportedPlatform`, a
reference that is not a secrets name `InvalidReference`, and every backend fault `Unavailable`,
without the backend's own text.

[Resolve a local secret](../guides/resolve-a-local-secret.md) shows each adapter in use.

## A Codex login

`CodexAuthFile` reads `/tokens/access_token` from a Codex `auth.json` on every request and judges
its JWT `exp` claim against the caller's clock. There is no default location: the caller passes an
absolute path, and a relative one is refused before anything is opened. `b10x-llm-tool-call`'s
`codex_auth_path` expands the usual one (`$CODEX_HOME/auth.json`, else `~/.codex/auth.json`) for a
caller that wants it.

- An expired or absent token is refused. Without `codex-renewal`, the operator renews the login by
  running `codex`, which owns the file.
- A login that is not a Codex login document, or whose token has no readable integer `exp`, is
  `SecretError::Malformed`. It is presented as `unauthorized` and **never falls back** to another
  account. A file that ends before its document does is `Unavailable`.

### Renewal, opt-in

With `codex-renewal`, `CodexAuthFile::renewing(CodexRenewal::new()?)` returns a resolver that renews
a login when its token expires within 15 minutes, before answering:

- one JSON `POST` of the refresh token to the token endpoint, never retried, bounded and without
  redirects;
- only the token values (and `last_refresh`) are written back, each at its own position, so every
  other byte of the file survives; the new file is written beside the old one with its mode and
  renamed over it only if the original still holds exactly the bytes the renewal read;
- a symlink or a hard-linked file is refused rather than replaced;
- a grant the endpoint refused, or one whose outcome is uncertain, is not sent again while the
  file is unchanged; any change to the file, such as `codex` logging in again, lifts that;
- the request and the answer are zeroized after use.

`CodexAuthFile::renew` renews once when the caller asks.

:::info[Planned: resolution through the Secrets library]
An optional resolver backed by the named storage of
[Secrets](https://beyond10x.github.io/secrets/) ([GitHub](https://github.com/beyond10x/secrets))
is planned and **not implemented**. It waits for that library's release, and it must not change
any route reference or add a dependency to the core crates. Until it exists, inject your own
`SecretResolver` for any store the adapters above do not cover.
:::
