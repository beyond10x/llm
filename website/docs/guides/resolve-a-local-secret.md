---
title: Resolve a local secret
description: Bind a route's opaque reference to a protected file or an explicitly chosen keychain entry.
---

# Resolve a local secret

A route catalog names a reference on its account; it never carries a value.

```toml
[[accounts]]
id = "lab"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "self-hosted"
secret_reference_id = "lab-token"
```

The embedding application binds that same name to a source. Nothing about the catalog changes when
the source does.

## A protected file

```rust
use llm_credentials::{file::FileResolver, SecretRef};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

let resolver = Arc::new(FileResolver::new(BTreeMap::from([
    (SecretRef::new("lab-token")?, PathBuf::from("/run/llm-secrets/lab-token")),
]))?);
```

Construction validates the explicit map without reading any file. Resolution on Linux walks
directory descriptors with `O_NOFOLLOW` and validates each opened object:

- Every parent is owned by the effective user or root, and is not group- or other-writable. A
  root-owned sticky directory such as `/tmp` is permitted.
- The leaf is a single-link regular file owned by the effective user or root, with no group or
  other permissions, no execute bit and no special mode bits. Use `0700` for the directory and
  `0600` or `0400` for the file.
- Symlinks in any component, hard links, directories, FIFOs, unsafe modes, relative paths and `..`
  are all refused.

Reads preserve raw bytes — empty values, NUL, invalid UTF-8 and trailing newlines included, with
nothing trimmed. It is the consumer that decides whether those bytes can be presented as
authentication. Reads are capped at 1 MiB, and a change to size, mtime or ctime during a read is
refused; rotate by atomically replacing the file so a reader sees a coherent old or new value.

:::note Linux only, deliberately
macOS ACL evaluation is outside this adapter's implemented checks, so it refuses macOS, Windows and
other platforms with `UnsupportedPlatform` rather than accepting a mode-bit-only approximation.
Use a keychain store or your own injected resolver there.
:::

Symlink-based projected volumes (a Kubernetes secret mount, for example) must be copied or mounted
into an explicitly protected regular file by the deployment before this adapter will read them.

## An explicitly chosen keychain

```rust
use llm_core::Id;
use llm_credentials::{keychain::{native_store, KeychainEntry, KeychainResolver}, SecretRef};
use std::{collections::BTreeMap, sync::Arc};

let resolver = Arc::new(KeychainResolver::new(native_store()?, BTreeMap::from([
    (SecretRef::new("lab-token")?, KeychainEntry {
        service: Id::new("my-application")?,
        entry: Id::new("lab-token")?,
    }),
]))?);
```

With only the `keychain` feature, inject a chosen `Arc<keyring_core::CredentialStore>` instead of
calling `native_store()`. The resolver asks that store for exactly the declared service and entry.
A missing entry returns `Missing`; every other backend error — locked, inaccessible, ambiguous —
maps to `Unavailable` without formatting backend diagnostics.

## The trust boundary

The trusted boundary is the kernel, the filesystem and root or effective-user writers. The adapter
does not defend against a compromised privileged writer or a filesystem that misreports
permissions, and a hostile writer can race the metadata checks — they are not a transactional
snapshot.

Neither adapter caches material between resolves. A local `SecretVersion` is an opaque SHA-256
content identity, not an issuer generation: the same bytes keep the same identity, and restoring
earlier bytes restores the earlier identity. It is redacted, non-serializable, and must never be
logged or used as a public fingerprint.

Mock-store tests and native-backend compilation do **not** establish live OS-service availability.
Linux needs an available Secret Service session, and the native service may require an unlock or
user interaction.
