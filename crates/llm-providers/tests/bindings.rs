use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, ErrorCode, Id, Protocol,
};
use llm_credentials::{
    CoordinatedResolver, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver,
    SecretVersion,
};
use llm_providers::{
    Account, ApiKeyHeader, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn document() -> BindingDocument {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("private-compute"),
        },
        Account {
            id: id("research"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Anonymous,
            billing_kind: BillingKind::SelfHosted,
            secret_reference_id: None,
            api_key_header: None,
        },
        Endpoint {
            id: id("gpu-7"),
            account_id: id("research"),
            base_url: BaseUrl::new("http://127.0.0.1:8000/proxy/v1").unwrap(),
        },
        ServedModel {
            id: id("code"),
            upstream_name: id("custom-org/CustomModel-27B:revision-4"),
        },
        ServingModel {
            id: id("local-code"),
            endpoint_id: id("gpu-7"),
            model_id: id("code"),
            protocol: Protocol::ChatCompletions,
            capabilities: Capabilities::text(32768, 8192),
        },
    )
}

fn authenticated(kind: AuthKind) -> BindingDocument {
    let mut doc = document();
    doc.account.auth_kind = kind;
    doc.account.secret_reference_id = Some(SecretRef::new("my-opaque-reference").unwrap());
    doc.account.api_key_header =
        (kind == AuthKind::ApiKey).then(|| ApiKeyHeader::new("X-Lab-Key").unwrap());
    doc
}

struct Resolver {
    material: Mutex<Vec<u8>>,
    generation: AtomicUsize,
    calls: AtomicUsize,
    refreshes: AtomicUsize,
    failure: Option<SecretError>,
}
impl Resolver {
    fn new(material: &[u8]) -> Self {
        Self {
            material: Mutex::new(material.to_vec()),
            generation: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            refreshes: AtomicUsize::new(0),
            failure: None,
        }
    }
}
impl SecretResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(reference.as_str(), "my-opaque-reference");
            if let Some(error) = self.failure {
                return Err(error);
            }
            Ok(ResolvedSecret {
                secret: Secret::new(self.material.lock().unwrap().clone())?,
                version: SecretVersion::new(format!(
                    "generation-{}",
                    self.generation.load(Ordering::SeqCst)
                ))?,
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

#[tokio::test]
async fn anonymous_vllm_binding_needs_no_secret_and_preserves_an_arbitrary_prefix() {
    let resolver = Resolver {
        failure: Some(SecretError::Unavailable),
        ..Resolver::new(b"unused")
    };
    let binding = document().bind().unwrap();
    assert_eq!(
        binding.request_url(),
        "http://127.0.0.1:8000/proxy/v1/chat/completions"
    );
    assert_eq!(
        binding.upstream_model(),
        "custom-org/CustomModel-27B:revision-4"
    );
    assert_eq!(binding.provenance().account.as_str(), "research");
    assert_eq!(binding.provenance().model.as_str(), "code");
    let (headers, generation) = binding
        .prepare_auth(&resolver, &Cancel::new())
        .await
        .unwrap()
        .into_parts();
    assert!(headers.is_empty());
    assert!(generation.is_none());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protocol_provider_auth_and_billing_are_independent_dimensions() {
    for (protocol, path) in [
        (Protocol::Responses, "responses"),
        (Protocol::Messages, "messages"),
        (Protocol::ChatCompletions, "chat/completions"),
    ] {
        for billing in [
            BillingKind::Metered,
            BillingKind::Subscription,
            BillingKind::SelfHosted,
        ] {
            for auth in [AuthKind::Anonymous, AuthKind::Bearer, AuthKind::ApiKey] {
                let mut doc = if auth == AuthKind::Anonymous {
                    document()
                } else {
                    authenticated(auth)
                };
                doc.serving.protocol = protocol;
                doc.account.billing_kind = billing;
                let binding = doc.bind().unwrap();
                assert_eq!(
                    binding.request_url(),
                    format!("http://127.0.0.1:8000/proxy/v1/{path}")
                );
                assert_eq!(binding.declaration().account.billing_kind, billing);
                let resolver = Resolver::new(b"example-credential");
                let (headers, version) = binding
                    .prepare_auth(&resolver, &Cancel::new())
                    .await
                    .unwrap()
                    .into_parts();
                match auth {
                    AuthKind::Anonymous => {
                        assert!(headers.is_empty());
                        assert!(version.is_none());
                    }
                    AuthKind::Bearer => {
                        assert_eq!(headers["authorization"], "Bearer example-credential");
                        assert_eq!(headers.len(), 1);
                    }
                    AuthKind::ApiKey => {
                        assert_eq!(headers["x-lab-key"], "example-credential");
                        assert_eq!(headers.len(), 1);
                    }
                    // Deliberately not independent: tied to Messages and subscription billing,
                    // and covered by tests/subscription_oauth.rs.
                    AuthKind::SubscriptionOauth => unreachable!("not iterated here"),
                }
                assert_eq!(
                    resolver.calls.load(Ordering::SeqCst),
                    usize::from(auth != AuthKind::Anonymous)
                );
                assert_eq!(resolver.refreshes.load(Ordering::SeqCst), 0);
            }
        }
    }
}

#[test]
fn contradictory_or_missing_authentication_is_refused_before_resolution() {
    let mut invalid = Vec::new();
    for kind in [AuthKind::Bearer, AuthKind::ApiKey] {
        let mut doc = authenticated(kind);
        doc.account.secret_reference_id = None;
        invalid.push(doc);
    }
    let mut doc = document();
    doc.account.secret_reference_id = Some(SecretRef::new("my-opaque-reference").unwrap());
    invalid.push(doc);
    let mut doc = document();
    doc.account.api_key_header = Some(ApiKeyHeader::new("x-api-key").unwrap());
    invalid.push(doc);
    let mut doc = authenticated(AuthKind::Bearer);
    doc.account.api_key_header = Some(ApiKeyHeader::new("x-api-key").unwrap());
    invalid.push(doc);
    let mut doc = authenticated(AuthKind::ApiKey);
    doc.account.api_key_header = None;
    invalid.push(doc);
    for doc in invalid {
        let error = doc.bind().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.dispatch, Dispatch::NotSent);
    }
}

#[test]
fn every_reference_and_capability_limit_is_checked() {
    for change in 0..5 {
        let mut doc = document();
        match change {
            0 => doc.account.provider_id = id("different"),
            1 => doc.endpoint.account_id = id("different"),
            2 => doc.serving.endpoint_id = id("different"),
            3 => doc.serving.model_id = id("different"),
            _ => doc.serving.capabilities.max_output_tokens = 32769,
        }
        assert_eq!(doc.bind().unwrap_err().code, ErrorCode::InvalidRequest);
    }
}

#[test]
fn urls_are_validated_when_deserialized_and_diagnostics_do_not_echo_credentials() {
    for url in [
        "https://name:do-not-echo@example.test/v1",
        "https://example.test/v1?token=do-not-echo",
        "https://example.test/v1#do-not-echo",
        "file:///do-not-echo",
        "https://example.test/\ndo-not-echo",
    ] {
        let error = serde_json::from_value::<BaseUrl>(json!(url)).unwrap_err();
        assert!(!error.to_string().contains("do-not-echo"));
    }
    assert!(BaseUrl::new(&format!("https://example.test/{}", "a".repeat(4096))).is_err());
    assert_eq!(
        BaseUrl::new("https://example.test/v1").unwrap(),
        BaseUrl::new("https://example.test/v1/").unwrap()
    );
    assert_eq!(
        BaseUrl::new("http://[::1]:8000/prefix%20name/v1")
            .unwrap()
            .as_str(),
        "http://[::1]:8000/prefix%20name/v1/"
    );
}

#[test]
fn api_key_header_cannot_override_routing_framing_or_content_negotiation() {
    for name in [
        "",
        "host",
        "CONTENT-LENGTH",
        "content-type",
        "transfer-encoding",
        "connection",
        "upgrade",
        "accept",
        "proxy-authorization",
        "cookie",
        "x-key\r\n",
    ] {
        assert!(ApiKeyHeader::new(name).is_err(), "accepted {name}");
        assert!(serde_json::from_value::<ApiKeyHeader>(json!(name)).is_err());
    }
    assert_eq!(
        ApiKeyHeader::new("X-API-Key").unwrap().as_str(),
        "x-api-key"
    );
}

#[test]
fn binding_documents_have_strict_versions_and_no_literal_secret_fields() {
    let encoded = serde_json::to_value(authenticated(AuthKind::Bearer)).unwrap();
    assert_eq!(encoded["format"], "llm.binding/1");
    serde_json::from_value::<BindingDocument>(encoded.clone())
        .unwrap()
        .bind()
        .unwrap();
    let mut future = encoded.clone();
    future["format"] = json!("llm.binding/2");
    assert!(serde_json::from_value::<BindingDocument>(future).is_err());
    for field in ["api_key", "access_token", "secret", "password"] {
        let mut invalid = encoded.clone();
        invalid["account"][field] = json!("not-a-reference");
        assert!(serde_json::from_value::<BindingDocument>(invalid).is_err());
    }
    let mut implicit = encoded;
    implicit["account"]
        .as_object_mut()
        .unwrap()
        .remove("auth_kind");
    assert!(serde_json::from_value::<BindingDocument>(implicit).is_err());
}

#[tokio::test]
async fn injected_coordinator_observes_rotation_without_mutating_the_binding() {
    let source = Arc::new(Resolver::new(b"first-credential"));
    let resolver = CoordinatedResolver::new(source.clone(), 4);
    let binding = authenticated(AuthKind::Bearer).bind().unwrap();
    let before = serde_json::to_value(binding.declaration()).unwrap();
    let auth = binding
        .prepare_auth(&resolver, &Cancel::new())
        .await
        .unwrap();
    assert!(!format!("{auth:?}").contains("first-credential"));
    let (first, version_one) = auth.into_parts();
    assert!(first["authorization"].is_sensitive());
    assert!(!format!("{first:?}").contains("first-credential"));
    *source.material.lock().unwrap() = b"rotated-credential".to_vec();
    source.generation.store(1, Ordering::SeqCst);
    let (second, version_two) = binding
        .prepare_auth(&resolver, &Cancel::new())
        .await
        .unwrap()
        .into_parts();
    assert_eq!(second["authorization"], "Bearer rotated-credential");
    assert_ne!(version_one, version_two);
    assert_eq!(serde_json::to_value(binding.declaration()).unwrap(), before);
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(source.refreshes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_material_and_missing_credentials_never_downgrade_or_refresh() {
    let binding = authenticated(AuthKind::Bearer).bind().unwrap();
    for bytes in [
        b"".to_vec(),
        b"token with spaces".to_vec(),
        b"secret\r\ninjected: value".to_vec(),
        vec![255],
        vec![b'a'; 16 * 1024 + 1],
    ] {
        let source = Resolver::new(&bytes);
        let error = binding
            .prepare_auth(&source, &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized);
        assert_eq!(error.dispatch, Dispatch::NotSent);
        assert!(!format!("{error:?}").contains("injected"));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(source.refreshes.load(Ordering::SeqCst), 0);
    }
    for error in [
        SecretError::Missing,
        SecretError::Expired,
        SecretError::Unavailable,
    ] {
        let source = Resolver {
            failure: Some(error),
            ..Resolver::new(b"unused")
        };
        assert!(binding.prepare_auth(&source, &Cancel::new()).await.is_err());
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(source.refreshes.load(Ordering::SeqCst), 0);
    }
}

struct PendingResolver {
    entered: tokio::sync::Notify,
    dropped: AtomicBool,
}
impl SecretResolver for PendingResolver {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            struct Guard<'a>(&'a AtomicBool);
            impl Drop for Guard<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _guard = Guard(&self.dropped);
            self.entered.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn cancellation_drops_pending_resolution_and_pre_cancel_skips_it() {
    let binding = authenticated(AuthKind::Bearer).bind().unwrap();
    let source = PendingResolver {
        entered: tokio::sync::Notify::new(),
        dropped: AtomicBool::new(false),
    };
    let cancel = Cancel::new();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(binding.prepare_auth(&source, &cancel), async {
            source.entered.notified().await;
            cancel.cancel();
        })
    })
    .await
    .unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(source.dropped.load(Ordering::SeqCst));
    let source = Resolver::new(b"unused");
    assert_eq!(
        binding
            .prepare_auth(&source, &cancel)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
}
