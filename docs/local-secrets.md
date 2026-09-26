# Explicit local secret sources

`llm-credentials` always accepts an application-owned `SecretResolver`. Its default feature set
has no file or native credential-store dependency. Local adapters are optional:

| Feature | Source | Availability |
| --- | --- | --- |
| `file` | Explicit absolute path per reference | Linux; other platforms return `UnsupportedPlatform` on resolve |
| `keychain` | Explicit injected `keyring_core::CredentialStore` and service/entry per reference | Any compatible injected store |
| `native-keychain` | Explicit native-store constructor, includes `keychain` | Linux Secret Service, macOS Keychain, Windows Credential Manager |

These adapters read existing material. They do not create entries, perform login, refresh tokens,
search vendor configuration directories, select another source when a reference is absent, or
modify the process-global keyring store. The application or operator provisions and rotates the
source separately. Entry values never belong in argv, TOML, tracing or diagnostics. A future
Connectors arbitrary-secret adapter can implement the same trait without changing route references.

## Bind a route reference to a protected file

The route catalog names the reference on its account (this is an account excerpt):

```toml
[[accounts]]
id = "lab"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "self-hosted"
secret_reference_id = "lab-token"
```

The embedding application binds that same name. With the `file` feature:

```rust
use llm_credentials::{file::FileResolver, SecretRef};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

let resolver = Arc::new(FileResolver::new(BTreeMap::from([
    (SecretRef::new("lab-token")?, PathBuf::from("/run/llm-secrets/lab-token")),
]))?);
// Inject `resolver` wherever a SecretResolver is accepted.
```

Construction validates the explicit map without reading files. Linux resolution walks directory
descriptors with `O_NOFOLLOW` and validates each opened object. Every parent is owned by the
effective user or root and is not writable by group/others; a root-owned sticky directory such as
`/tmp` is permitted. The leaf must be a single-link regular file owned by the effective user or
root, with no group/other permissions, execution permission or special mode bits. Typically use
`0700` for a private directory and `0600` or `0400` for its file. Symlinks in any component,
hard links, directories, FIFOs and unsafe modes are refused. Relative paths and `..` are refused.
Symlink-based projected volumes must be copied or mounted into an explicitly protected regular
file by the application deployment before using this adapter.

Reads preserve raw bytes, including empty values, NUL, invalid UTF-8 and trailing newlines.
Nothing is trimmed. The consumer validates whether those bytes can be presented as authentication.
Reads are capped at 1 MiB, and changes to size, modification time or change time during a read
are refused. Rotate with an atomic replacement in the protected directory to provide a coherent
old or new value. The trusted boundary is the kernel, filesystem and root/effective-user writers;
the adapter does not protect against a compromised privileged writer or a filesystem that lies
about its permissions. A hostile writer can race metadata checks, so they are not a transactional
snapshot guarantee.

macOS ACL evaluation is outside this file adapter's implemented protection checks. It therefore
refuses macOS, Windows and other unsupported platforms instead of accepting a mode-bit-only
approximation. Use a keychain store or an injected resolver there. Apple's filesystem security
model includes ACLs as well as BSD permissions. [Apple documentation](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/FileSystemDetails/FileSystemDetails.html)

## Bind a reference to a chosen keychain

With `native-keychain`, an embedding explicitly selects the native store:

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

With only `keychain`, inject a chosen `Arc<keyring_core::CredentialStore>` instead of calling
`native_store()`. Tests use `keyring_core::mock::Store` and never open the user's native store.
The underlying API supports this explicit-store composition. [Keyring core](https://github.com/open-source-cooperative/keyring-core)

The resolver asks the selected store for exactly the declared service and entry. A missing entry
returns `Missing`; all other backend errors, including locked, inaccessible and ambiguous entries,
map to `Unavailable` without formatting backend diagnostics. The native OS service may require an
unlock or user interaction. Linux needs an available Secret Service session. Mock tests and native
backend compilation do not establish live OS-service availability or its support for every byte
sequence. The selected store implementation is a trusted dependency of the embedding.

## Rotation, memory and cancellation

Neither local adapter caches material between resolves. Local `SecretVersion` is an opaque SHA-256
content identity, not an issuer's monotonic generation: the same bytes keep the same identity,
changed bytes change it, and restoring earlier bytes restores the earlier identity. It is redacted
and nonserializable, and must never be logged or used as a public fingerprint. Refresh returns
`RefreshUnsupported`. A caller may observe independently rotated bytes through `CoordinatedResolver`.

Returned material uses the same zeroized, redacted `Secret` as injected resolvers. Each adapter
accepts at most 4096 bindings and allows at most eight blocking OS reads at once. A cancelled async
waiter does not release its permit until the OS call finishes. Blocking OS calls cannot be forcibly
cancelled through this interface; a stuck service can occupy capacity or delay runtime shutdown.
An embedding that needs hard process deadlines must isolate that service boundary itself.

Run `task check` for runtime tests, optional native-backend compilation, ESS behavior and drift,
and AEP validation. The [verification record](verification/local-secrets.md) states the measured
coverage and remaining qualification boundaries.
