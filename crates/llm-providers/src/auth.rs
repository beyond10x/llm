use crate::Binding;
use http::{HeaderMap, HeaderName, HeaderValue, header::AUTHORIZATION};
use llm_core::{AuthKind, Cancel, Error, ErrorCode};
use llm_credentials::{ReferenceError, SecretError, SecretRef, SecretResolver, SecretVersion};
use std::fmt;
use zeroize::Zeroizing;

/// Request-local sensitive headers and the exact credential generation used, without secret caching.
pub struct PreparedAuth {
    headers: HeaderMap,
    generation: Option<SecretVersion>,
}

impl PreparedAuth {
    pub fn into_parts(self) -> (HeaderMap, Option<SecretVersion>) {
        (self.headers, self.generation)
    }
}

impl fmt::Debug for PreparedAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedAuth")
            .field("headers", &"[REDACTED]")
            .field("authenticated", &self.generation.is_some())
            .finish()
    }
}

impl Binding {
    /// Resolve this account's reference once. Does not refresh, resend or search another source.
    /// # Errors
    /// Refuses cancellation, missing/unavailable credentials and invalid HTTP token material.
    pub async fn prepare_auth(
        &self,
        resolver: &dyn SecretResolver,
        cancel: &Cancel,
    ) -> Result<PreparedAuth, Error> {
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        let account = &self.declaration().account;
        let mut headers = HeaderMap::new();
        let Some(reference) = &account.secret_reference_id else {
            // Binding construction already proves that this account is explicitly anonymous.
            return Ok(PreparedAuth {
                headers,
                generation: None,
            });
        };
        let credential = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(Error::cancelled()),
            reply = resolver.resolve(reference) => reply.map_err(|e| secret_error(reference, e))?,
        };
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        // A token file written by an editor or `echo` ends in one line terminator; that one is
        // not part of the token. Nothing else is removed.
        let material = credential.secret.expose();
        let material = material
            .strip_suffix(b"\r\n")
            .or_else(|| material.strip_suffix(b"\n"))
            .unwrap_or(material);
        if material.is_empty()
            || material.len() > 16 * 1024
            || !material.iter().all(u8::is_ascii_graphic)
        {
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "credential is not a bounded HTTP token",
            ));
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(material.len() + 7));
        let name = match account.auth_kind {
            AuthKind::Bearer => {
                bytes.extend_from_slice(b"Bearer ");
                AUTHORIZATION
            }
            AuthKind::ApiKey => {
                let name = account
                    .api_key_header
                    .as_ref()
                    .ok_or_else(|| Error::invalid("missing API-key header"))?;
                HeaderName::from_bytes(name.as_str().as_bytes())
                    .map_err(|_| Error::invalid("invalid API-key header"))?
            }
            AuthKind::Anonymous => {
                return Err(Error::invalid("anonymous binding has a credential"));
            }
        };
        bytes.extend_from_slice(material);
        let mut value = HeaderValue::from_bytes(&bytes).map_err(|_| {
            Error::new(
                ErrorCode::Unauthorized,
                "credential is not a valid HTTP token",
            )
        })?;
        value.set_sensitive(true);
        headers.insert(name, value);
        Ok(PreparedAuth {
            headers,
            generation: Some(credential.version),
        })
    }
}

/// The refusal names the account's reference, so an operator can tell which credential failed;
/// never its path, variable or value. A missing, expired or malformed credential is the caller's
/// to fix and is `unauthorized`, which no fallback takes; every other failure is `unavailable`.
fn secret_error(reference: &SecretRef, error: SecretError) -> Error {
    let code = if matches!(
        error,
        SecretError::Missing | SecretError::Expired | SecretError::Malformed
    ) {
        ErrorCode::Unauthorized
    } else {
        ErrorCode::Unavailable
    };
    // SecretError carries only fixed, safe diagnostics, never a backend exception or secret, and
    // the reference is the operator's non-secret lookup name.
    Error::new(
        code,
        ReferenceError::new(error, reference.clone()).to_string(),
    )
}
