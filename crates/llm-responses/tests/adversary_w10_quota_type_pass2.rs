//! Adversary, wave 2026-10-20-w10, unit http, pass 2 (story:quota-429-is-final). The Responses
//! quota rule now also reads `error.type`. Two questions, each as a table of 429 bodies run
//! through the Responses client:
//!
//! 1. Can a rate limit read final through the new half of the rule: a type that only mentions a
//!    quota word, a type that is not a string, a quota type outside `error`, a quota word in the
//!    prose?
//! 2. Does the rule class every 429 body the official Codex CLI's own tests drive as Codex does
//!    (`openai/codex` 8196bcdc)? Terminal there: `api_bridge.rs:192`-`238` and `error.rs:410`-`437`;
//!    retried: `RetryLimit`, `api_bridge.rs:241`.
//!
//! Local sockets only.

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
use serde_json::{Value, json};
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

/// `body` answered as an HTTP 429 before any stream; the turn's refusal.
async fn as_429(body: &Value) -> Error {
    let body = body.to_string();
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
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Say hello")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("a 429 is a refusal");
    server.await.expect("the fixture server");
    assert_eq!(sink.events(), [], "{error:?}");
    error
}

/// Whether the client read `body` as a final quota rather than a retriable rate limit.
fn read_final(error: &Error, body: &Value) -> bool {
    match (error.code, error.may_retry()) {
        (ErrorCode::Refused, false) => true,
        (ErrorCode::RateLimited, true) => false,
        other => panic!("{body}: neither class: {other:?} {error:?}"),
    }
}

/// Rate limits that only brush the new half of the rule stay retriable.
#[tokio::test]
async fn a_rate_limit_near_a_quota_type_stays_retriable() {
    for body in [
        // A type that contains a quota word but is not one of the three.
        json!({"error": {"type": "insufficient_quota_rate", "code": "rate_limit_exceeded"}}),
        json!({"error": {"type": "usage_limit", "code": "rate_limit_exceeded"}}),
        json!({"error": {"type": " usage_limit_reached", "code": "rate_limit_exceeded"}}),
        json!({"error": {"type": "usage_limit_reached ", "code": "rate_limit_exceeded"}}),
        // The quota word in the prose only.
        json!({"error": {"type": "requests", "code": "rate_limit_exceeded",
            "message": "insufficient_quota usage_limit_reached usage_not_included"}}),
        // A type that is not a string.
        json!({"error": {"type": ["insufficient_quota"], "code": "rate_limit_exceeded"}}),
        json!({"error": {"type": {"name": "usage_limit_reached"}, "code": "rate_limit_exceeded"}}),
        json!({"error": {"type": true, "code": "rate_limit_exceeded"}}),
        json!({"error": {"type": null, "code": "rate_limit_exceeded"}}),
        // A quota type at the top level, or beside `error` rather than in it.
        json!({"type": "usage_limit_reached", "error": {"type": "requests", "code": "rate_limit_exceeded"}}),
        json!({"type": "insufficient_quota", "error": {"code": "rate_limit_exceeded"}}),
        json!({"error": {"code": "rate_limit_exceeded", "details": {"type": "insufficient_quota"}}}),
        // `error` not an object.
        json!({"error": "insufficient_quota"}),
        json!({"error": ["usage_limit_reached"]}),
        json!([{"error": {"type": "insufficient_quota"}}]),
    ] {
        let error = as_429(&body).await;
        assert!(!read_final(&error, &body), "{body} read final: {error:?}");
    }
}

/// Every 429 body the Codex CLI's tests drive, with the class Codex gives it: its source and
/// whether Codex reads it terminal.
fn codex_fixtures() -> Vec<(&'static str, Value, bool)> {
    vec![
        (
            "core/tests/suite/client.rs:3575",
            json!({"error": {"type": "usage_limit_reached", "message": "limit reached",
                "resets_at": 1_704_067_242, "plan_type": "pro"}}),
            true,
        ),
        (
            "core/tests/suite/turn_error_details_tests.rs:85",
            json!({"error": {"type": "usage_limit_reached", "resets_at": 1_800_000_000,
                "limit_window_minutes": 10_080}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:547",
            json!({"error": {"type": "usage_limit_reached", "plan_type": "pro",
                "limit_window_minutes": "300"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:87",
            json!({"error": {"type": "insufficient_quota", "code": "insufficient_quota"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:88",
            json!({"error": {"type": "usage_limit_reached", "code": "usage_limit_reached"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:86",
            json!({"error": {"type": "rate_limit_exceeded", "code": "rate_limit_exceeded"}}),
            false,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:505",
            json!({"error": {"type": "insufficient_quota"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:506",
            json!({"error": {"code": "insufficient_quota"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:507",
            json!({"error": {"code": "credit_balance_exhausted"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:508",
            json!({"error": {"code": "organization_spend_limit_exceeded"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:509",
            json!({"error": {"code": "project_spend_limit_exceeded"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:510",
            json!({"error": {"code": "organization_usage_limit_exceeded"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:511",
            json!({"error": {"type": "rate_limit_error", "code": "rate_limit_exceeded"}}),
            false,
        ),
        (
            "codex-api/src/api_bridge_tests.rs:512",
            json!({"error": {"type": "rate_limit_error", "code": "slow_down"}}),
            false,
        ),
        (
            "core/tests/suite/retry_after.rs:356",
            json!({"error": {"code": "rate_limit_exceeded"}}),
            false,
        ),
        // api_bridge.rs:225-235: a quota type or a quota code, whatever stands beside it.
        (
            "codex-api/src/api_bridge.rs:225 (type beside a rate-limit code)",
            json!({"error": {"type": "insufficient_quota", "code": "rate_limit_exceeded"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge.rs:226 (code beside a rate-limit type)",
            json!({"error": {"type": "rate_limit_error", "code": "insufficient_quota"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge.rs:192 (usage limit beside a rate-limit code)",
            json!({"error": {"type": "usage_limit_reached", "code": "rate_limit_exceeded"}}),
            true,
        ),
        (
            "codex-api/src/api_bridge.rs:192 (a usage-limit name as the code only)",
            json!({"error": {"code": "usage_limit_reached"}}),
            false,
        ),
    ]
}

#[tokio::test]
async fn every_codex_429_fixture_reads_as_codex_reads_it() {
    for (source, body, terminal) in codex_fixtures() {
        let error = as_429(&body).await;
        assert_eq!(
            read_final(&error, &body),
            terminal,
            "{source}: {body}: {error:?}"
        );
    }
}
