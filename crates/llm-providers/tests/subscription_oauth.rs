//! story:anthropic-access, Harness parity C8 and C21 (Harness `harness-credential/src/oauth.rs:110`,
//! `:148`; `harness-messages/src/lib.rs:202`-`205`, at Harness `origin/main` `3169042f`).
//!
//! A `subscription-oauth` account holds a caller-supplied subscription token behind its secret
//! reference. llm offers no login and reads no login file: the token is whatever the caller's
//! resolver answers for the declared reference. At this layer it is one sensitive
//! `authorization: Bearer` header; the Messages-specific presentation (the OAuth beta header and
//! the client preamble) is llm-messages' (tests/subscription.rs there). `bind` refuses every
//! declaration that would send the token anywhere else or bill it as metered API use.
//!
//! The documents are parsed from JSON on purpose: the spelling `subscription-oauth` is the
//! declaration vocabulary under test. No credential store, file or network is involved.
use http::header::AUTHORIZATION;
use llm_core::{AuthKind, BoxFuture, Cancel, ErrorCode};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_providers::{Binding, BindingDocument};
use serde_json::{Value, json};
use std::sync::Mutex;

const TOKEN: &str = "llm-fixture-private-marker-subscription";

/// One subscription account on Messages, billed as the subscription it is.
fn document() -> Value {
    json!({
        "format": "llm.binding/1",
        "provider": {"id": "anthropic", "category": "hosted"},
        "account": {
            "id": "operator",
            "provider_id": "anthropic",
            "auth_kind": "subscription-oauth",
            "billing_kind": "subscription",
            "secret_reference_id": "operator-subscription"
        },
        "endpoint": {
            "id": "messages",
            "account_id": "operator",
            "base_url": "https://messages.example.invalid/v1"
        },
        "model": {"id": "internal-model", "upstream_name": "example/Model-Revision"},
        "serving": {
            "id": "subscription-model",
            "endpoint_id": "messages",
            "model_id": "internal-model",
            "protocol": "messages",
            "capabilities": {
                "tools": true, "tool_choice": true, "temperature": true, "top_p": false,
                "reasoning_efforts": [], "context_window": 200_000, "max_output_tokens": 8192
            }
        }
    })
}

/// Parses a declaration that must parse: every refusal below is `bind`'s, not the parser's.
fn parse(document: Value) -> BindingDocument {
    serde_json::from_value(document)
        .expect("`subscription-oauth` is an account auth kind the declaration vocabulary spells")
}

fn bind(document: Value) -> Result<Binding, llm_core::Error> {
    parse(document).bind()
}

/// Answers the fixture token and records every reference it was asked for.
#[derive(Default)]
struct Recording {
    asked: Mutex<Vec<String>>,
}

impl SecretResolver for Recording {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.asked
            .lock()
            .unwrap()
            .push(reference.as_str().to_owned());
        Box::pin(async {
            Ok(ResolvedSecret {
                secret: Secret::new(TOKEN.as_bytes().to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

/// A refusal `bind` must make before any resolver could be consulted.
fn refused(document: Value, message: &str) {
    let error = bind(document).expect_err("the declaration is refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest, "{error}");
    assert_eq!(error.message, message);
}

#[test]
fn the_auth_kind_is_spelled_subscription_oauth_and_is_neither_bearer_nor_api_key() {
    let kind: AuthKind = serde_json::from_value(json!("subscription-oauth"))
        .expect("`subscription-oauth` is an account auth kind");
    assert_eq!(
        serde_json::to_value(kind).unwrap(),
        json!("subscription-oauth")
    );
    assert_ne!(kind, AuthKind::Bearer);
    assert_ne!(kind, AuthKind::ApiKey);
    assert_ne!(kind, AuthKind::Anonymous);
}

#[tokio::test]
async fn a_subscription_oauth_account_presents_its_token_once_as_a_sensitive_bearer() {
    let binding = bind(document()).expect("a subscription account on Messages binds");
    assert_eq!(
        binding.request_url(),
        "https://messages.example.invalid/v1/messages"
    );
    let resolver = Recording::default();
    let (headers, generation) = binding
        .prepare_auth(&resolver, &Cancel::new())
        .await
        .expect("the token is prepared")
        .into_parts();

    // Resolved once, through the declared reference and nothing else (C21: never a file).
    assert_eq!(
        *resolver.asked.lock().unwrap(),
        ["operator-subscription".to_owned()]
    );
    assert_eq!(headers.len(), 1, "{headers:?}");
    let value = headers.get(AUTHORIZATION).expect("an authorization header");
    assert_eq!(value.as_bytes(), format!("Bearer {TOKEN}").as_bytes());
    assert!(value.is_sensitive());
    assert!(generation.is_some());
    // The protocol-specific half of the presentation is not this layer's to add.
    assert!(headers.get("anthropic-beta").is_none());
    assert!(headers.get("x-api-key").is_none());
}

#[test]
fn a_subscription_oauth_account_billed_as_anything_but_its_subscription_is_refused() {
    for billing in ["metered", "self-hosted"] {
        let mut document = document();
        document["account"]["billing_kind"] = json!(billing);
        refused(
            document,
            "subscription OAuth accounts require subscription billing",
        );
    }
}

#[test]
fn a_subscription_oauth_account_on_another_protocol_is_refused() {
    for protocol in ["responses", "chat-completions"] {
        let mut document = document();
        document["serving"]["protocol"] = json!(protocol);
        refused(
            document,
            "subscription OAuth accounts are served only over Messages",
        );
    }
}

#[test]
fn a_subscription_oauth_account_with_an_api_key_header_is_refused() {
    let mut document = document();
    document["account"]["api_key_header"] = json!("x-api-key");
    refused(
        document,
        "exactly API-key accounts require an API-key header",
    );
}

#[test]
fn a_subscription_oauth_account_without_a_reference_is_refused() {
    let mut document = document();
    document["account"]
        .as_object_mut()
        .unwrap()
        .remove("secret_reference_id");
    refused(
        document,
        "authenticated accounts require a secret reference",
    );
}

/// The same account declared as a plain bearer is a different binding: opaque state and usage
/// bound under one never return to the other.
#[test]
fn a_subscription_oauth_binding_is_not_the_bearer_binding_of_the_same_account() {
    let subscription = bind(document()).expect("binds");
    let mut bearer = document();
    bearer["account"]["auth_kind"] = json!("bearer");
    let bearer = bind(bearer).expect("a bearer account under subscription billing binds");
    assert_ne!(
        subscription.provenance().binding_revision,
        bearer.provenance().binding_revision
    );
}
