//! Adversary pass, story:anthropic-access (wave 2026-10-05-w48).
//!
//! The providers half: the subscription token never reaches a diagnostic (Debug of the prepared
//! auth, Debug of its headers, the refusal of a malformed token), and the billing/protocol
//! refusal holds for every combination and every declaration order, not only the two the unit
//! named. Fixture documents and an injected resolver only.
use llm_core::{BoxFuture, Cancel};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_providers::BindingDocument;
use serde_json::{Map, Value, json};

const TOKEN: &str = "adversary-private-subscription-marker";

struct Fixed(&'static str);

impl SecretResolver for Fixed {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        let material = self.0;
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(material.as_bytes().to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

fn document(billing: &str, protocol: &str) -> Value {
    json!({
        "format": "llm.binding/1",
        "provider": {"id": "anthropic", "category": "hosted"},
        "account": {
            "id": "operator",
            "provider_id": "anthropic",
            "auth_kind": "subscription-oauth",
            "billing_kind": billing,
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
            "protocol": protocol,
            "capabilities": {
                "tools": true, "tool_choice": true, "temperature": true, "top_p": false,
                "reasoning_efforts": [], "context_window": 200_000, "max_output_tokens": 8192
            }
        }
    })
}

/// Reverses key order at every level, so a refusal cannot depend on which field is read first.
fn reversed(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            let mut keys: Vec<_> = map.keys().collect();
            keys.reverse();
            for key in keys {
                out.insert(key.clone(), reversed(&map[key]));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[test]
fn only_subscription_billing_over_messages_binds_in_any_declaration_order() {
    for billing in ["metered", "subscription", "self-hosted"] {
        for protocol in ["messages", "responses", "chat-completions"] {
            for doc in [
                document(billing, protocol),
                reversed(&document(billing, protocol)),
            ] {
                let parsed: BindingDocument = serde_json::from_value(doc).unwrap();
                let bound = parsed.bind();
                assert_eq!(
                    bound.is_ok(),
                    billing == "subscription" && protocol == "messages",
                    "{billing} over {protocol}: {:?}",
                    bound.as_ref().err()
                );
            }
        }
    }
}

#[tokio::test]
async fn the_subscription_token_reaches_no_diagnostic() {
    let binding = serde_json::from_value::<BindingDocument>(document("subscription", "messages"))
        .unwrap()
        .bind()
        .unwrap();
    let prepared = binding
        .prepare_auth(&Fixed(TOKEN), &Cancel::new())
        .await
        .unwrap();
    let debug = format!("{prepared:?} {binding:?}");
    assert!(!debug.contains(TOKEN), "{debug}");
    let (headers, _) = prepared.into_parts();
    let debug = format!("{headers:?}");
    assert!(!debug.contains(TOKEN), "{debug}");

    // A token the transport cannot carry is refused without being echoed.
    for malformed in [
        "adversary-private subscription-marker",
        "adversary-private-subscription-marker\u{7f}",
    ] {
        let leaked: &'static str = Box::leak(malformed.to_owned().into_boxed_str());
        let error = binding
            .prepare_auth(&Fixed(leaked), &Cancel::new())
            .await
            .expect_err("refused");
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains("adversary-private"), "{shown}");
    }
}
