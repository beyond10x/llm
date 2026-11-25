//! Adversary cases for `JsonPointerResolver` (wave 2026-10-05-w22, pass 1).
//!
//! The oracle is `serde_json::Value::pointer` followed by `as_str`, which is exactly what Harness
//! `SubscriptionToken::at_pointer` evaluates (`harness-credential/src/oauth.rs`), so a pointer
//! llm resolves differently from Harness on any of these documents is a parity break. The
//! documented differences are excluded on purpose: an empty string is `Missing` in llm (Harness
//! refuses it as empty), and a pointer with an invalid `~` escape is refused at construction.
#![cfg(feature = "json-pointer")]
use llm_core::BoxFuture;
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
    pointer::JsonPointerResolver,
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

fn reference() -> SecretRef {
    SecretRef::new("explicit").unwrap()
}

struct Document(Vec<u8>);
impl SecretResolver for Document {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(self.0.clone())?,
                version: SecretVersion::new("document".to_owned())?,
            })
        })
    }
}

fn bound(document: &[u8], pointer: &str) -> JsonPointerResolver {
    JsonPointerResolver::new(
        Arc::new(Document(document.to_vec())),
        BTreeMap::from([(reference(), pointer.to_owned())]),
    )
    .unwrap()
}

/// Harness's answer: the nonempty string at `pointer`, or nothing.
fn oracle(document: &str, pointer: &str) -> Option<Vec<u8>> {
    let value: Value = serde_json::from_str(document).ok()?;
    let token = value.pointer(pointer)?.as_str()?;
    (!token.is_empty()).then(|| token.as_bytes().to_vec())
}

#[tokio::test]
async fn the_pointer_selects_exactly_what_harness_selects() {
    let documents = [
        // RFC 6901 section 4: `~01` is the member `~1`, never `/`. Decoding `~0` before `~1`
        // would turn it into `/` and select the other member.
        r#"{"~1":"tilde-one","/":"slash"}"#,
        r#"{"a/b":{"c~d":"escaped"},"a":{"b":"unescaped"}}"#,
        r#"{"list":["zero","one","two"],"01":"leading-zero-member"}"#,
        r#"{"":{"":"empty-members"}}"#,
        // A member name written with JSON escapes is the unescaped name.
        r#"{"a/b":"escaped-key","~":"escaped-tilde"}"#,
        // A repeated member: serde_json keeps the last.
        r#"{"t":"first","t":"last"}"#,
        r#"{"t":"first","t":17}"#,
        r#" [ {"t" : "spaced" } , "x" ] "#,
        r#""whole-document""#,
        r#"{"n":null,"b":true,"o":{},"a":[],"num":1.5}"#,
    ];
    let pointers = [
        "",
        "/",
        "//",
        "/~01",
        "/~1",
        "/~0",
        "/a~1b/c~0d",
        "/a/b",
        "/list/0",
        "/list/2",
        "/list/3",
        "/list/01",
        "/list/-",
        "/list/+1",
        "/list/1 ",
        "/01",
        "/0",
        "/0/t",
        "/1",
        "/t",
        "/n",
        "/b",
        "/o",
        "/a",
        "/num",
        "/~01/x",
    ];
    let mut compared = 0;
    for document in documents {
        for pointer in pointers {
            let expected = oracle(document, pointer);
            let actual = bound(document.as_bytes(), pointer)
                .resolve(&reference())
                .await
                .map(|resolved| resolved.secret.expose().to_vec());
            match (&expected, &actual) {
                (Some(expected), Ok(actual)) => assert_eq!(
                    String::from_utf8_lossy(actual),
                    String::from_utf8_lossy(expected),
                    "{document} at {pointer:?}"
                ),
                (None, Err(_)) => {}
                _ => panic!(
                    "{document} at {pointer:?}: Harness selects {:?}, llm returns {:?}",
                    expected.as_deref().map(String::from_utf8_lossy),
                    actual.as_ref().map(|bytes| String::from_utf8_lossy(bytes))
                ),
            }
            compared += 1;
        }
    }
    assert_eq!(compared, documents.len() * pointers.len());
}

// docs/local-secrets.md: "A pointer that is neither empty nor starts with `/` ... is refused";
// the empty pointer is valid RFC 6901 and selects the whole document. No test constructs a
// resolver with it, so refusing it at construction would pass the unit's suite.
#[tokio::test]
async fn an_empty_pointer_is_accepted_and_selects_the_whole_document() {
    let resolver = bound(br#""llm-fixture-whole""#, "");
    assert_eq!(
        resolver
            .resolve(&reference())
            .await
            .unwrap()
            .secret
            .expose(),
        b"llm-fixture-whole"
    );
}

// pointer.rs documents `TooManyReferences` above 4096 bindings; no test reaches it.
#[test]
fn more_than_4096_pointers_are_refused_and_4096_are_accepted() {
    let bindings = |count: usize| -> BTreeMap<SecretRef, String> {
        (0..count)
            .map(|index| {
                (
                    SecretRef::new(format!("reference-{index}")).unwrap(),
                    "/t".to_owned(),
                )
            })
            .collect()
    };
    assert!(JsonPointerResolver::new(Arc::new(Document(b"{}".to_vec())), bindings(4096)).is_ok());
    assert_eq!(
        JsonPointerResolver::new(Arc::new(Document(b"{}".to_vec())), bindings(4097)).unwrap_err(),
        SecretError::TooManyReferences
    );
}

// The document is the source's material, up to 1 MiB. Walking it must not overflow the stack
// however deeply it nests, and a pointer through it must still answer.
#[tokio::test]
async fn a_deeply_nested_document_does_not_overflow() {
    let depth = 500_000;
    let mut document = "[".repeat(depth);
    document.push_str(&"]".repeat(depth));
    let resolver = bound(document.as_bytes(), "/0/0/0");
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Malformed
    );
    let mut nested = r#"{"a":"#.repeat(2_000);
    nested.push_str(r#""llm-fixture-deep""#);
    nested.push_str(&"}".repeat(2_000));
    let pointer = "/a".repeat(2_000);
    assert_eq!(
        bound(nested.as_bytes(), &pointer)
            .resolve(&reference())
            .await
            .unwrap()
            .secret
            .expose(),
        b"llm-fixture-deep"
    );
}
