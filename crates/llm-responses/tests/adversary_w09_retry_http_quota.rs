//! Adversary pass 1 on story:retry-classes-harness-retried (wave 2026-10-20-w09).
//!
//! The story's decision: "A class a provider page says not to retry stays final ... (for example
//! a 429 that means an exhausted quota, not a rate limit)". `OpenAI`'s error codes page lists
//! each billing code as an HTTP 429 entry ("429 - Credit balance exhausted", "429 - Organization
//! spend limit reached", ...) and says "Retrying billing, spend, or quota errors won't restore API
//! access." The unit made these codes final inside a stream. As the 429 before any stream, the
//! path that page describes, the transport marks every 429 retriable without reading the body
//! (`crates/llm-http/src/transport.rs:339`), so routing retries the target and then falls back.
//! `docs/design.md` said so and deferred the transport change. The coordinator kept the transport
//! as it was (correction round 1), and this case recorded that behaviour until
//! story:quota-429-is-final, which flipped it: the transport now reads a 429's bounded body for
//! the Responses client's quota rule, and each code reads as a final refusal.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Error, ErrorCode, Id, Item, Model,
    Protocol, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding(base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id("example/Small-Model"),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: true,
                top_p: true,
                reasoning_efforts: vec!["medium".to_owned()],
                context_window: 32_768,
                max_output_tokens: 2_048,
                output_streaming: true,
                input_during_output: false,
            },
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

struct Resolver;

impl SecretResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

/// Answers one request with a 429 carrying `code`, closes, and returns the turn's refusal.
async fn quota_turn(code: &str) -> Error {
    let body = serde_json::json!({"error": {
        "message": "SECRET-UPSTREAM-TEXT", "type": "insufficient_quota", "param": null, "code": code
    }})
    .to_string();
    let head = format!(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket
            .write_all(head.as_bytes())
            .await
            .expect("head written");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("body written");
        let _ = socket.shutdown().await;
    });
    let client = ResponsesClient::new(
        binding(&url),
        HttpClient::new(Limits {
            response_headers: Duration::from_secs(20),
            idle: Duration::from_secs(20),
            total: Duration::from_secs(30),
        })
        .expect("bounded transport"),
        Arc::new(Resolver),
    )
    .expect("a Responses binding");
    let request = TurnRequest::new("small", vec![Item::user("Say hello")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("a 429 is a refusal");
    server.await.expect("the fixture server");
    assert_eq!(sink.events(), [], "`{code}`");
    error
}

/// Flipped by story:quota-429-is-final: each billing code answered as a 429 before the stream is
/// a final `Refused`, as the same code inside a stream is, and its prose reaches no diagnostic.
#[tokio::test]
async fn an_exhausted_quota_answered_as_a_429_before_the_stream_is_final() {
    for code in [
        "insufficient_quota",
        "credit_balance_exhausted",
        "organization_spend_limit_exceeded",
        "project_spend_limit_exceeded",
        "organization_usage_limit_exceeded",
    ] {
        let error = quota_turn(code).await;
        assert!(
            !format!("{error:?}").contains("SECRET-UPSTREAM-TEXT"),
            "`{code}`: {error:?}"
        );
        assert_eq!(error.code, ErrorCode::Refused, "`{code}`: {error:?}");
        assert!(
            !error.may_retry(),
            "`{code}`: an exhausted quota is final before the stream too: {error:?}"
        );
    }
}
