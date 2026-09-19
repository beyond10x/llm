#![forbid(unsafe_code)]

//! Injected secret resolution and caller-managed credential refresh; optional backend adapters.
//!
//! Secret references and caller-injected custody. No login, ambient lookup or persistent writes.

use llm_core::{BoxFuture, Id};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as AsyncMutex;
use zeroize::Zeroizing;

/// A non-secret, operator-defined lookup name. The resolver decides where it lives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(Id);
impl SecretRef {
    /// # Errors
    /// Refuses empty, oversized or non-printable lookup identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        Id::new(value)
            .map(Self)
            .map_err(|_| SecretError::InvalidReference)
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Deliberately has no caller-controlled diagnostic string: backend errors can contain secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("invalid secret reference")]
    InvalidReference,
    #[error("secret is unavailable")]
    Unavailable,
    #[error("secret reference was not found")]
    Missing,
    #[error("secret has expired")]
    Expired,
    #[error("caller resolver does not support refresh")]
    RefreshUnsupported,
    #[error("caller resolver did not replace the rejected generation")]
    RefreshRejected,
    #[error("refresh outcome is uncertain; caller must reconcile credential state")]
    RefreshUncertain,
    #[error("secret value or version exceeds its size bound")]
    TooLarge,
    #[error("too many secret references are coordinated")]
    TooManyReferences,
}

/// Arbitrary short-lived bytes; Debug is redacted and storage is zeroized on drop.
///
/// ```compile_fail
/// let secret = llm_credentials::Secret::new(b"example".to_vec()).unwrap();
/// serde_json::to_string(&secret).unwrap(); // secret material is not serializable
/// ```
///
/// ```compile_fail
/// let secret = llm_credentials::Secret::new(b"example".to_vec()).unwrap();
/// println!("{secret}"); // secret material has no Display implementation
/// ```
pub struct Secret(Zeroizing<Vec<u8>>);
impl Secret {
    /// # Errors
    /// Refuses a secret larger than 1 MiB. Empty/multiline/binary secrets are allowed here;
    /// the consuming provider validates its own authentication presentation.
    pub fn new(value: Vec<u8>) -> Result<Self, SecretError> {
        let value = Zeroizing::new(value);
        if value.len() > 1024 * 1024 {
            return Err(SecretError::TooLarge);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

/// Caller-owned generation identity. Stable for one value and changed on every rotation.
/// This also has redacted Debug; neither a version nor the credential can be serialized.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretVersion(Zeroizing<String>);
impl SecretVersion {
    /// # Errors
    /// Requires a nonempty generation identifier of at most 1024 bytes.
    pub fn new(version: String) -> Result<Self, SecretError> {
        let version = Zeroizing::new(version);
        if version.is_empty() || version.len() > 1024 {
            return Err(SecretError::TooLarge);
        }
        Ok(Self(version))
    }
}
impl fmt::Debug for SecretVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretVersion([REDACTED])")
    }
}

#[derive(Debug)]
pub struct ResolvedSecret {
    pub secret: Secret,
    pub version: SecretVersion,
}

/// Implemented by the embedding application, local adapter, or future Connectors adapter.
pub trait SecretResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>>;
    /// Caller-owned renewal. Implementations must persist a new version before returning success.
    /// A cancelled refresh may have taken effect: subsequent callers resolve before refreshing.
    fn refresh<'a>(
        &'a self,
        _reference: &'a SecretRef,
        _rejected: &'a SecretVersion,
    ) -> BoxFuture<'a, Result<(), SecretError>> {
        Box::pin(async { Err(SecretError::RefreshUnsupported) })
    }
}

#[derive(Default)]
struct RefreshState {
    attempted: Option<(SecretVersion, Result<(), SecretError>)>,
}

/// Serializes per-reference renewal; never caches secret material between requests.
/// The map is bounded. A caller must still limit authenticated resend to one qualified retry.
pub struct CoordinatedResolver {
    source: Arc<dyn SecretResolver>,
    references: Mutex<BTreeMap<SecretRef, Arc<AsyncMutex<RefreshState>>>>,
    max_references: usize,
}

impl CoordinatedResolver {
    pub fn new(source: Arc<dyn SecretResolver>, max_references: usize) -> Self {
        Self {
            source,
            references: Mutex::new(BTreeMap::new()),
            max_references,
        }
    }

    fn state(&self, reference: &SecretRef) -> Result<Arc<AsyncMutex<RefreshState>>, SecretError> {
        let mut references = self
            .references
            .lock()
            .map_err(|_| SecretError::Unavailable)?;
        if let Some(state) = references.get(reference) {
            return Ok(state.clone());
        }
        if references.len() >= self.max_references {
            return Err(SecretError::TooManyReferences);
        }
        let state = Arc::new(AsyncMutex::new(RefreshState::default()));
        references.insert(reference.clone(), state.clone());
        Ok(state)
    }

    /// # Errors
    /// Returns the resolver's typed failure; no other source is tried.
    pub async fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret, SecretError> {
        let state = self.state(reference)?;
        let _guard = state.lock().await;
        self.source.resolve(reference).await
    }

    /// # Errors
    /// Refuses unsupported/failed refresh, unchanged versions, and exhausted reference capacity.
    pub async fn refresh(
        &self,
        reference: &SecretRef,
        rejected: &SecretVersion,
    ) -> Result<(), SecretError> {
        let state = self.state(reference)?;
        let mut guard = state.lock().await;
        // Always observe external rotation first. A completed concurrent refresh is not repeated.
        let current = self.source.resolve(reference).await?;
        if current.version != *rejected {
            return Ok(());
        }
        drop(current);
        if let Some((version, result)) = &guard.attempted
            && version == rejected
        {
            return *result;
        }
        // Persist uncertainty before awaiting a mutation. Dropping this future must not let the
        // next waiter blindly repeat an authorization-server mutation on the same generation.
        guard.attempted = Some((rejected.clone(), Err(SecretError::RefreshUncertain)));
        let result = match self.source.refresh(reference, rejected).await {
            Ok(()) => self.source.resolve(reference).await.and_then(|new| {
                if new.version == *rejected {
                    Err(SecretError::RefreshRejected)
                } else {
                    Ok(())
                }
            }),
            Err(error) => Err(error),
        };
        guard.attempted = Some((rejected.clone(), result));
        result
    }
}
