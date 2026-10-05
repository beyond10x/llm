use crate::SecretError;
#[cfg(any(
    feature = "codex-auth-file",
    feature = "environment",
    feature = "json-pointer",
    feature = "keychain",
    target_os = "linux"
))]
use crate::{ResolvedSecret, Secret, SecretVersion};
#[cfg(any(
    feature = "codex-auth-file",
    feature = "environment",
    feature = "json-pointer",
    feature = "keychain",
    target_os = "linux"
))]
use sha2::{Digest, Sha256};
#[cfg(any(
    feature = "codex-auth-file",
    feature = "environment",
    feature = "json-pointer",
    feature = "keychain",
    target_os = "linux"
))]
use std::fmt::Write;
#[cfg(any(feature = "codex-auth-file", feature = "file", feature = "keychain"))]
use std::sync::Arc;
#[cfg(any(feature = "codex-auth-file", feature = "file", feature = "keychain"))]
use tokio::sync::Semaphore;

#[cfg(any(
    feature = "environment",
    feature = "file",
    feature = "json-pointer",
    feature = "keychain"
))]
pub const MAX_BINDINGS: usize = 4096;
#[cfg(any(feature = "codex-auth-file", feature = "file", feature = "keychain"))]
pub const MAX_BLOCKING_READS: usize = 8;

#[cfg(any(
    feature = "codex-auth-file",
    feature = "environment",
    feature = "json-pointer",
    feature = "keychain",
    target_os = "linux"
))]
pub fn resolved(secret: Secret) -> Result<ResolvedSecret, SecretError> {
    // Read-only sources have no issuer generation. Equal material identifies the
    // same value; restoring old bytes restores that identity. Never log this hash.
    let mut version = zeroize::Zeroizing::new(String::from("value-sha256:"));
    for byte in Sha256::digest(secret.expose()) {
        write!(version, "{byte:02x}").map_err(|_| SecretError::Unavailable)?;
    }
    Ok(ResolvedSecret {
        secret,
        version: SecretVersion::new(std::mem::take(&mut *version))?,
    })
}

#[cfg(any(feature = "codex-auth-file", feature = "file", feature = "keychain"))]
pub async fn blocking<T: Send + 'static>(
    permits: Arc<Semaphore>,
    operation: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    let permit = permits
        .acquire_owned()
        .await
        .map_err(|_| SecretError::Unavailable)?;
    tokio::task::spawn_blocking(move || {
        // Cancellation of the waiting future cannot release a permit while its
        // non-interruptible OS call is still executing.
        let _permit = permit;
        operation()
    })
    .await
    .map_err(|_| SecretError::Unavailable)?
}

#[cfg(all(
    test,
    any(feature = "codex-auth-file", feature = "file", feature = "keychain")
))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_waiter_retains_capacity_until_os_call_finishes() {
        let permits = Arc::new(Semaphore::new(1));
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_permits = permits.clone();
        let task = tokio::spawn(async move {
            blocking(task_permits, move || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .await
        });
        entered_rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(permits.available_permits(), 0);
        release_tx.send(()).unwrap();
        let permit = permits.acquire().await.unwrap();
        drop(permit);
        assert_eq!(permits.available_permits(), 1);
    }
}
