//! An explicitly selected keyring store and explicit service/entry names, with no global default.
use crate::{ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, local};
use keyring_core::CredentialStore;
use llm_core::{BoxFuture, Id};
use std::{collections::BTreeMap, fmt, sync::Arc};
use tokio::sync::Semaphore;

/// The service and entry are operator metadata, never secret values or a search pattern.
#[derive(Clone)]
pub struct KeychainEntry {
    pub service: Id,
    pub entry: Id,
}
impl fmt::Debug for KeychainEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeychainEntry([EXPLICIT])")
    }
}

pub struct KeychainResolver {
    store: Arc<CredentialStore>,
    bindings: BTreeMap<SecretRef, KeychainEntry>,
    permits: Arc<Semaphore>,
}
impl KeychainResolver {
    /// Construct a resolver without calling the injected store. The embedding owns store choice.
    /// # Errors
    /// Refuses more than 4096 explicitly bound references.
    pub fn new(
        store: Arc<CredentialStore>,
        bindings: BTreeMap<SecretRef, KeychainEntry>,
    ) -> Result<Self, SecretError> {
        if bindings.len() > local::MAX_BINDINGS {
            return Err(SecretError::TooManyReferences);
        }
        Ok(Self {
            store,
            bindings,
            permits: Arc::new(Semaphore::new(local::MAX_BLOCKING_READS)),
        })
    }
}
impl fmt::Debug for KeychainResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeychainResolver")
            .field("bindings", &self.bindings.len())
            .finish_non_exhaustive()
    }
}
fn failure(error: keyring_core::Error) -> SecretError {
    match error {
        keyring_core::Error::NoEntry => SecretError::Missing,
        other => {
            drop(other); // Discard backend diagnostics without formatting them.
            SecretError::Unavailable
        }
    }
}
impl SecretResolver for KeychainResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            let binding = self
                .bindings
                .get(reference)
                .ok_or(SecretError::Missing)?
                .clone();
            let store = self.store.clone();
            local::blocking(self.permits.clone(), move || {
                let entry = store
                    .build(binding.service.as_str(), binding.entry.as_str(), None)
                    .map_err(failure)?;
                let secret = Secret::new(entry.get_secret().map_err(failure)?)?;
                local::resolved(secret)
            })
            .await
        })
    }
}

/// Explicitly opt into the native OS credential store. This never changes the process default.
/// # Errors
/// Refuses initialization errors and unsupported platforms. Resolving may require OS unlock.
#[cfg(feature = "native-keychain")]
pub fn native_store() -> Result<Arc<CredentialStore>, SecretError> {
    #[cfg(target_os = "linux")]
    {
        zbus_secret_service_keyring_store::Store::new()
            .map(|store| store as Arc<CredentialStore>)
            .map_err(failure)
    }
    #[cfg(target_os = "macos")]
    {
        apple_native_keyring_store::keychain::Store::new()
            .map(|store| store as Arc<CredentialStore>)
            .map_err(failure)
    }
    #[cfg(target_os = "windows")]
    {
        windows_native_keyring_store::Store::new()
            .map(|store| store as Arc<CredentialStore>)
            .map_err(failure)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(SecretError::UnsupportedPlatform)
    }
}
