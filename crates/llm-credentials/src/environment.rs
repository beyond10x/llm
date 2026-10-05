//! Caller-named environment variables. The caller binds each reference to one variable name;
//! nothing else in the environment is consulted, and no name is known to this crate.
//!
//! Values are raw bytes, as for files: nothing is trimmed or decoded. The variable is read on
//! every resolve and never cached. Read-only: `refresh` returns `RefreshUnsupported`.
use crate::{
    ReferenceError, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, local,
};
use llm_core::BoxFuture;
use std::{collections::BTreeMap, fmt};

/// No environment access occurs until a known reference is resolved.
pub struct EnvironmentResolver {
    bindings: BTreeMap<SecretRef, String>,
}

impl EnvironmentResolver {
    /// # Errors
    /// `InvalidReference` for an empty variable name or one holding `=` or NUL;
    /// `TooManyReferences` above 4096 bindings.
    pub fn new(bindings: BTreeMap<SecretRef, String>) -> Result<Self, SecretError> {
        if bindings.len() > local::MAX_BINDINGS {
            return Err(SecretError::TooManyReferences);
        }
        if bindings
            .values()
            .any(|name| name.is_empty() || name.contains(['=', '\0']))
        {
            return Err(SecretError::InvalidReference);
        }
        Ok(Self { bindings })
    }

    /// Resolves as [`SecretResolver::resolve`] does, with a refusal that names `reference`.
    /// A future like every other source's `read`, so callers treat sources alike; it is ready
    /// at once, because reading the environment does not wait.
    ///
    /// # Errors
    /// The same refusal kinds as `resolve`; the error never carries the variable name or value.
    pub fn read(
        &self,
        reference: &SecretRef,
    ) -> impl Future<Output = Result<ResolvedSecret, ReferenceError>> + use<> {
        std::future::ready(
            self.lookup(reference)
                .map_err(|kind| ReferenceError::new(kind, reference.clone())),
        )
    }

    /// `Missing` for an unbound reference or an unset variable; `TooLarge` above 1 MiB;
    /// `Unavailable` for a value that is not Unicode on a platform without byte-string values.
    fn lookup(&self, reference: &SecretRef) -> Result<ResolvedSecret, SecretError> {
        let name = self.bindings.get(reference).ok_or(SecretError::Missing)?;
        let value = std::env::var_os(name).ok_or(SecretError::Missing)?;
        #[cfg(unix)]
        let bytes = std::os::unix::ffi::OsStringExt::into_vec(value);
        #[cfg(not(unix))]
        let bytes = value
            .into_string()
            .map_err(|_| SecretError::Unavailable)?
            .into_bytes();
        // `Secret::new` owns and zeroizes the buffer, and refuses one above the size bound.
        local::resolved(Secret::new(bytes)?)
    }
}

impl fmt::Debug for EnvironmentResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentResolver")
            .field("bindings", &self.bindings.len())
            .finish_non_exhaustive()
    }
}

impl SecretResolver for EnvironmentResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move { self.lookup(reference) })
    }
}
