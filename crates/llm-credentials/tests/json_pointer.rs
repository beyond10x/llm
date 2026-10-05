//! A token at a caller-named RFC 6901 pointer of a JSON document another source holds (Harness
//! `SubscriptionToken::at_pointer`). The pointer is the caller's: llm knows no store's layout.
#![cfg(feature = "json-pointer")]
use llm_core::BoxFuture;
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
    pointer::JsonPointerResolver,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

const CANARY: &str = "llm-fixture-private-marker";
const POINTER: &str = "/claudeAiOauth/accessToken";

fn reference() -> SecretRef {
    SecretRef::new("explicit").unwrap()
}

/// An injected source holding one document, or one refusal, for any reference.
struct Document {
    answer: Mutex<Result<Vec<u8>, SecretError>>,
    resolves: AtomicUsize,
    refreshes: AtomicUsize,
}
impl Document {
    fn new(answer: Result<&[u8], SecretError>) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(answer.map(<[u8]>::to_vec)),
            resolves: AtomicUsize::new(0),
            refreshes: AtomicUsize::new(0),
        })
    }
    fn set(&self, document: &[u8]) {
        *self.answer.lock().unwrap() = Ok(document.to_vec());
    }
}
impl SecretResolver for Document {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.resolves.fetch_add(1, Ordering::SeqCst);
            let document = self.answer.lock().unwrap().clone()?;
            Ok(ResolvedSecret {
                version: SecretVersion::new(format!("document-{}", document.len()))?,
                secret: Secret::new(document)?,
            })
        })
    }
    fn refresh<'a>(
        &'a self,
        _: &'a SecretRef,
        _: &'a SecretVersion,
    ) -> BoxFuture<'a, Result<(), SecretError>> {
        Box::pin(async move {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

fn bound(source: &Arc<Document>, pointer: &str) -> JsonPointerResolver {
    JsonPointerResolver::new(
        source.clone(),
        BTreeMap::from([(reference(), pointer.to_owned())]),
    )
    .unwrap()
}

#[tokio::test]
async fn a_token_at_a_caller_named_pointer_resolves_to_exactly_that_string() {
    let source = Document::new(Ok(br#"{"claudeAiOauth":{"accessToken":"llm-fixture-private-marker-access","refreshToken":"llm-fixture-private-marker-refresh","expiresAt":1}}"#));
    let resolver = bound(&source, POINTER);
    let first = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(first.secret.expose(), b"llm-fixture-private-marker-access");
    assert_eq!(
        resolver.read(&reference()).await.unwrap().secret.expose(),
        b"llm-fixture-private-marker-access"
    );

    // The version identifies the token presented, not the document around it: a sibling that
    // changes alone keeps it, a changed token changes it.
    source.set(br#"{"claudeAiOauth":{"accessToken":"llm-fixture-private-marker-access","refreshToken":"rotated","expiresAt":2}}"#);
    let sibling = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(first.version, sibling.version);
    source.set(br#"{"claudeAiOauth":{"accessToken":"llm-fixture-private-marker-renewed"}}"#);
    let renewed = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(
        renewed.secret.expose(),
        b"llm-fixture-private-marker-renewed"
    );
    assert_ne!(first.version, renewed.version);

    // Read-only: renewal is the store owner's, and the source behind it is never asked to.
    assert_eq!(
        resolver.refresh(&reference(), &renewed.version).await,
        Err(SecretError::RefreshUnsupported)
    );
    assert_eq!(source.refreshes.load(Ordering::SeqCst), 0);
    let rendered = format!("{renewed:?} {resolver:?}");
    assert!(!rendered.contains(CANARY), "{rendered}");
}

#[tokio::test]
async fn rfc6901_escapes_and_array_indices_are_followed() {
    for (document, pointer) in [
        (&br#"{"a/b":{"c~d":"llm-fixture-token"}}"#[..], "/a~1b/c~0d"),
        (br#"{"list":["other","llm-fixture-token"]}"#, "/list/1"),
        (br#"{"":{"x":"llm-fixture-token"}}"#, "//x"),
    ] {
        let resolver = bound(&Document::new(Ok(document)), pointer);
        assert_eq!(
            resolver
                .resolve(&reference())
                .await
                .unwrap()
                .secret
                .expose(),
            b"llm-fixture-token",
            "{pointer}"
        );
    }
}

#[tokio::test]
async fn pointer_refusals_are_typed_and_name_only_the_reference() {
    for (document, expected) in [
        (&br#"{"claudeAiOauth":{}}"#[..], SecretError::Missing),
        (
            br#"{"claudeAiOauth":{"accessToken":""}}"#,
            SecretError::Missing,
        ),
        (
            br#"{"claudeAiOauth":{"accessToken":17}}"#,
            SecretError::Malformed,
        ),
        (
            br#"{"claudeAiOauth":{"accessToken":null}}"#,
            SecretError::Malformed,
        ),
        (
            br#"{"claudeAiOauth":"llm-fixture-private-marker"}"#,
            SecretError::Missing,
        ),
        (b"llm-fixture-private-marker\n", SecretError::Malformed),
        (
            br#"{"claudeAiOauth":{"accessToken":"llm-fixture-private-marker"}"#,
            SecretError::Malformed,
        ),
    ] {
        let resolver = bound(&Document::new(Ok(document)), POINTER);
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            expected,
            "{}",
            String::from_utf8_lossy(document)
        );
        let error = resolver.read(&reference()).await.unwrap_err();
        assert_eq!(error.kind(), expected);
        assert_eq!(error.reference(), &reference());
        let rendered = format!("{error} {error:?}");
        assert!(rendered.contains("explicit"), "{rendered}");
        assert!(!rendered.contains(CANARY), "{rendered}");
    }
    // The source's own refusal is passed on unchanged.
    for refusal in [
        SecretError::Missing,
        SecretError::UnsafeSource,
        SecretError::TooLarge,
    ] {
        let resolver = bound(&Document::new(Err(refusal)), POINTER);
        assert_eq!(resolver.resolve(&reference()).await.unwrap_err(), refusal);
    }
    // A reference bound to no pointer is missing, and the source is never consulted for it.
    let source = Document::new(Ok(br#"{"claudeAiOauth":{"accessToken":"token"}}"#));
    let resolver = bound(&source, POINTER);
    let other = SecretRef::new("unbound").unwrap();
    assert_eq!(
        resolver.resolve(&other).await.unwrap_err(),
        SecretError::Missing
    );
    assert_eq!(resolver.read(&other).await.unwrap_err().reference(), &other);
    assert_eq!(source.resolves.load(Ordering::SeqCst), 0);
}

#[test]
fn a_pointer_that_is_not_rfc6901_is_refused_at_construction() {
    for pointer in ["claudeAiOauth/accessToken", "/bad~2escape", "/trailing~"] {
        assert_eq!(
            JsonPointerResolver::new(
                Document::new(Ok(b"{}")),
                BTreeMap::from([(reference(), pointer.to_owned())]),
            )
            .unwrap_err(),
            SecretError::InvalidReference,
            "{pointer}"
        );
    }
}

#[cfg(all(feature = "file", target_os = "linux"))]
#[tokio::test]
async fn a_subscription_login_layout_resolves_through_the_protected_file_adapter() {
    use llm_credentials::file::FileResolver;
    use std::{fs, os::unix::fs::PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("credentials.json");
    fs::write(
        &path,
        b"{\"claudeAiOauth\":{\"accessToken\":\"llm-fixture-private-marker-access\"}}\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let file = FileResolver::new(BTreeMap::from([(reference(), path)])).unwrap();
    let resolver = JsonPointerResolver::new(
        Arc::new(file),
        BTreeMap::from([(reference(), POINTER.to_owned())]),
    )
    .unwrap();
    assert_eq!(
        resolver
            .resolve(&reference())
            .await
            .unwrap()
            .secret
            .expose(),
        b"llm-fixture-private-marker-access"
    );
}
