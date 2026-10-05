//! A token at a caller-named RFC 6901 JSON pointer of a document another resolver returns.
//!
//! The pointer is the caller's: which member a credential store keeps its access token in is that
//! store's business, so this crate knows no layout. The wrapped source supplies the document and
//! its protections (a [`crate::file::FileResolver`] for a protected file, for instance).
//!
//! The token is the JSON string at the pointer, raw: nothing is trimmed. Its version is the
//! content identity of that string, so a sibling member changing alone keeps the version. The
//! document is walked without copying any member but the selected one; a token written with JSON
//! escapes is unescaped through the parser's own buffer, which is freed without zeroizing.
//! Read-only: `refresh` returns `RefreshUnsupported` and never reaches the wrapped source.
use crate::{
    ReferenceError, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, local,
};
use llm_core::BoxFuture;
use serde_json::value::RawValue;
use std::{borrow::Cow, collections::BTreeMap, fmt, sync::Arc};
use zeroize::Zeroizing;

pub struct JsonPointerResolver {
    source: Arc<dyn SecretResolver>,
    pointers: BTreeMap<SecretRef, String>,
}

impl JsonPointerResolver {
    /// Binds each reference to a pointer into the document `source` resolves for that same
    /// reference. No I/O happens until a request.
    ///
    /// # Errors
    /// `InvalidReference` for a pointer that is not RFC 6901 (nonempty without a leading `/`, or
    /// `~` not followed by `0` or `1`); `TooManyReferences` above 4096 bindings.
    pub fn new(
        source: Arc<dyn SecretResolver>,
        pointers: BTreeMap<SecretRef, String>,
    ) -> Result<Self, SecretError> {
        if pointers.len() > local::MAX_BINDINGS {
            return Err(SecretError::TooManyReferences);
        }
        if !pointers.values().all(|pointer| valid(pointer)) {
            return Err(SecretError::InvalidReference);
        }
        Ok(Self { source, pointers })
    }

    /// Resolves as [`SecretResolver::resolve`] does, with a refusal that names `reference`.
    ///
    /// # Errors
    /// The same refusal kinds as `resolve`; the error never carries the document or the token.
    pub async fn read(&self, reference: &SecretRef) -> Result<ResolvedSecret, ReferenceError> {
        self.lookup(reference)
            .await
            .map_err(|kind| ReferenceError::new(kind, reference.clone()))
    }

    /// `Missing` for an unbound reference (the source is not consulted), nothing at the pointer or
    /// an empty string there; `Malformed` for a document that is not UTF-8 JSON or a value that is
    /// not a string (a configuration error, refused like a missing credential); any refusal of
    /// the wrapped source unchanged.
    async fn lookup(&self, reference: &SecretRef) -> Result<ResolvedSecret, SecretError> {
        let pointer = self.pointers.get(reference).ok_or(SecretError::Missing)?;
        let document = self.source.resolve(reference).await?;
        let mut token = select(document.secret.expose(), pointer)?;
        local::resolved(Secret::new(std::mem::take(&mut *token))?)
    }
}

fn valid(pointer: &str) -> bool {
    if !(pointer.is_empty() || pointer.starts_with('/')) {
        return false;
    }
    let bytes = pointer.as_bytes();
    bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| *byte != b'~' || matches!(bytes.get(index + 1), Some(b'0' | b'1')))
}

/// The string at `pointer` of `document`, walked through borrowed raw values.
fn select(document: &[u8], pointer: &str) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    let text = std::str::from_utf8(document).map_err(|_| SecretError::Malformed)?;
    let mut current: &RawValue = serde_json::from_str(text).map_err(|_| SecretError::Malformed)?;
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        let next = match current.get().trim_start().as_bytes().first() {
            Some(b'{') => serde_json::from_str::<BTreeMap<Cow<'_, str>, &RawValue>>(current.get())
                .map_err(|_| SecretError::Malformed)?
                .get(segment.as_str())
                .copied(),
            Some(b'[') => {
                let items = serde_json::from_str::<Vec<&RawValue>>(current.get())
                    .map_err(|_| SecretError::Malformed)?;
                index(&segment).and_then(|index| items.get(index).copied())
            }
            _ => None,
        };
        current = next.ok_or(SecretError::Missing)?;
    }
    if !current.get().trim_start().starts_with('"') {
        return Err(SecretError::Malformed);
    }
    let token: Cow<'_, str> =
        serde_json::from_str(current.get()).map_err(|_| SecretError::Malformed)?;
    let token = match token {
        Cow::Borrowed(token) => Zeroizing::new(token.as_bytes().to_vec()),
        Cow::Owned(token) => Zeroizing::new(token.into_bytes()),
    };
    if token.is_empty() {
        return Err(SecretError::Missing);
    }
    Ok(token)
}

/// An RFC 6901 array index: `0`, or digits without a leading zero. `-` and anything else select
/// nothing.
fn index(segment: &str) -> Option<usize> {
    if segment.is_empty()
        || !segment.bytes().all(|byte| byte.is_ascii_digit())
        || (segment.len() > 1 && segment.starts_with('0'))
    {
        return None;
    }
    segment.parse().ok()
}

impl fmt::Debug for JsonPointerResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonPointerResolver")
            .field("pointers", &self.pointers.len())
            .finish_non_exhaustive()
    }
}

impl SecretResolver for JsonPointerResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(self.lookup(reference))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_follow_rfc6901() {
        assert_eq!(index("0"), Some(0));
        assert_eq!(index("12"), Some(12));
        for segment in ["", "-", "01", "+1", "1a"] {
            assert_eq!(index(segment), None, "{segment}");
        }
    }

    #[test]
    fn an_escaped_token_is_unescaped_and_an_object_member_is_not_a_token() {
        assert_eq!(select(br#"{"t":"abc"}"#, "/t").unwrap().as_slice(), b"abc");
        assert_eq!(
            select(br#"{"t":{"u":"x"}}"#, "/t").unwrap_err(),
            SecretError::Malformed
        );
        assert_eq!(select(br#""whole""#, "").unwrap().as_slice(), b"whole");
    }
}
