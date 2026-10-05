//! Resolution through the `secrets` library's storage port (`secrets.storage`).
//!
//! A [`SecretRef`] is read as a secrets [`SecretName`](storage::SecretName) in one [`Scope`]
//! configured on the resolver, and [`SecretResolver::resolve`] reads it from a
//! [`SecretStorage`]. The resolver never writes, lists or falls back to another source.
//!
//! Every port refusal becomes an existing [`SecretError`]: `not-found` is
//! [`SecretError::Missing`], `denied` is [`SecretError::UnsafeSource`], `unsupported` is
//! [`SecretError::UnsupportedPlatform`], `invalid-name` is [`SecretError::InvalidReference`],
//! `too-large` is [`SecretError::TooLarge`], and every other code is
//! [`SecretError::Unavailable`]. A backend whose capabilities lack read is
//! [`SecretError::UnsupportedPlatform`] and is never called. A reference that is not a secrets
//! name is refused before any backend is asked. The port's codes carry no backend text, so
//! neither does a refusal here.
//!
//! The version is the backend's, so every write, also of the same bytes, is a new version. A
//! backend that gives no version gets content identity, as the read-only local adapters do.
//! [`SecretResolver::refresh`] re-reads and is [`SecretError::RefreshRejected`] while the version
//! is the rejected one.
//!
//! [`SecretsResolver::keychain`] is the library's keychain backend behind its local authorizer,
//! over a store the embedding chooses. Nothing here reads or sets `keyring_core`'s process-wide
//! default store, and no native store is linked by this feature.
use crate::{ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion};
use keyring_core::CredentialStore;
use llm_core::BoxFuture;
use sha2::{Digest, Sha256};
use std::{fmt, fmt::Write, sync::Arc};

pub use secrets_core::{authorize, storage};
pub use secrets_keychain as keychain;

use storage::{Address, Capability, Scope, SecretName, SecretStorage, StorageError, Target};

/// A port refusal as the existing [`SecretError`] code. The match has no wildcard.
fn failure(error: StorageError) -> SecretError {
    match error {
        StorageError::NotFound => SecretError::Missing,
        StorageError::Denied => SecretError::UnsafeSource,
        StorageError::Unsupported => SecretError::UnsupportedPlatform,
        StorageError::InvalidName => SecretError::InvalidReference,
        StorageError::TooLarge => SecretError::TooLarge,
        StorageError::Unavailable | StorageError::Conflict => SecretError::Unavailable,
    }
}

/// A bounded identity: the SHA-256 of `bytes` under `label`. Never log it.
fn identity(label: &str, bytes: &[u8]) -> Result<SecretVersion, SecretError> {
    let mut version = zeroize::Zeroizing::new(format!("{label}-sha256:"));
    for byte in Sha256::digest(bytes) {
        write!(version, "{byte:02x}").map_err(|_| SecretError::Unavailable)?;
    }
    SecretVersion::new(std::mem::take(&mut *version))
}

/// Resolves references by name in one scope of a `secrets` storage.
pub struct SecretsResolver {
    storage: Arc<dyn SecretStorage>,
    scope: Scope,
}

impl SecretsResolver {
    /// A resolver over `storage` in `scope`. Does not call the storage.
    pub fn new(storage: Arc<dyn SecretStorage>, scope: Scope) -> Self {
        Self { storage, scope }
    }

    /// The library's keychain backend over `store`, behind its local authorizer. Does not call
    /// the store; the embedding owns the store choice.
    pub fn keychain(store: Arc<CredentialStore>, scope: Scope) -> Self {
        Self::new(
            Arc::new(authorize::Authorized::local(
                keychain::KeychainBackend::new(store),
            )),
            scope,
        )
    }

    /// The scope every reference is read in.
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    fn target(&self, reference: &SecretRef) -> Result<Target, SecretError> {
        let name =
            SecretName::parse(reference.as_str()).map_err(|_| SecretError::InvalidReference)?;
        Ok(Target::unbound(Address {
            scope: self.scope.clone(),
            name,
        }))
    }

    async fn read(&self, reference: &SecretRef) -> Result<ResolvedSecret, SecretError> {
        let target = self.target(reference)?;
        // As the library's mount routing does (`secrets-federation`, `require`): a backend that
        // does not offer read is refused before it is called.
        if !self.storage.capabilities().contains(&Capability::Read) {
            return Err(SecretError::UnsupportedPlatform);
        }
        let revealed = self.storage.read(&target).await.map_err(failure)?;
        let secret = Secret::new(revealed.value.expose().to_vec())?;
        let version = match &revealed.version {
            Some(version) => identity("storage-version", version.as_str().as_bytes())?,
            None => identity("value", secret.expose())?,
        };
        Ok(ResolvedSecret { secret, version })
    }
}

impl fmt::Debug for SecretsResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretsResolver")
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

impl SecretResolver for SecretsResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(self.read(reference))
    }

    fn refresh<'a>(
        &'a self,
        reference: &'a SecretRef,
        rejected: &'a SecretVersion,
    ) -> BoxFuture<'a, Result<(), SecretError>> {
        Box::pin(async move {
            if self.read(reference).await?.version == *rejected {
                Err(SecretError::RefreshRejected)
            } else {
                Ok(())
            }
        })
    }
}
