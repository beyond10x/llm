//! Adversary, wave 2026-10-20-w10, unit http, pass 1 (story:quota-429-is-final).
//!
//! The Responses quota rule reads `error.code` only. The official Codex CLI, at the commit
//! `docs/design.md` already cites for the Responses error codes (`openai/codex` 8196bcdc,
//! `codex-rs/codex-api/src/api_bridge.rs:185`-`238`), reads a 429's `error.type` as well:
//! `usage_limit_reached` and `usage_not_included` (the Codex backend this client serves through
//! `llm-tool-call`'s `CODEX_BASE_URL`), and `insufficient_quota` named by `error.type` alone, are
//! terminal there (`codex-rs/protocol/src/error.rs:410`-`437`, `retry_delay` returns `None`);
//! its own test `map_api_error_distinguishes_http_quota_errors_from_rate_limits`
//! (`codex-rs/codex-api/src/api_bridge_tests.rs:503`) pins `{"type": "insufficient_quota"}`
//! without a code as a quota. Inside a stream, this crate's decoder already reads every such
//! document as a final `Refused` (any code but the three transient ones, an absent code
//! included). As a 429 before the stream the same document stays a retriable `RateLimited`: the
//! asymmetry the story set out to remove. Local sockets only.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item,
    Model, Protocol, TurnRequest, VecSink,
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

const PROSE: &str = "SECRET-UPSTREAM-TEXT";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CREATED: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n";

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

/// Answers one request with `head` and `body`, closes, and returns the turn's refusal and how
/// many requests arrived.
async fn failed_turn(path: &str, head: Vec<u8>, body: String) -> (Error, VecSink, usize) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!(
        "http://{}{path}",
        listener.local_addr().expect("an address")
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket.write_all(&head).await.expect("head written");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("body written");
        let _ = socket.shutdown().await;
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        1 + usize::from(resent)
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
        .expect_err("the turn failed");
    let requests = server.await.expect("the fixture server");
    (error, sink, requests)
}

/// `error` answered as an HTTP 429 before any stream.
async fn as_429(path: &str, error: &Value) -> (Error, VecSink, usize) {
    let body = json!({ "error": error }).to_string();
    let head = format!(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    failed_turn(path, head.into_bytes(), body).await
}

/// `error` reported inside a stream before any output, as `response.failed`.
async fn in_stream(error: &Value) -> (Error, VecSink, usize) {
    let payload = json!({"type": "response.failed", "response": {
        "id": "resp_1", "status": "failed", "error": error
    }});
    failed_turn(
        "/v1",
        SSE_HEAD.to_vec(),
        format!("{CREATED}data: {payload}\n\n"),
    )
    .await
}

fn assert_final(error: &Error, case: &str) {
    assert_eq!(error.code, ErrorCode::Refused, "{case}: {error:?}");
    assert!(
        !error.retriable && !error.may_retry(),
        "{case} is final in the reference client: {error:?}"
    );
    assert!(!format!("{error:?} {error}").contains(PROSE), "{case}");
}

/// Codex's own fixture: `{"type": "insufficient_quota"}` with no code is a quota. Inside a stream
/// this crate reads it final; as a 429 it must read final too.
#[tokio::test]
async fn a_quota_named_by_its_error_type_is_final_as_a_429_as_it_is_inside_a_stream() {
    let quota = json!({"type": "insufficient_quota", "code": null, "message": PROSE});
    let (streamed, sink, _) = in_stream(&quota).await;
    assert_final(&streamed, "insufficient_quota by type, inside a stream");
    assert_eq!(sink.events(), []);

    let (refused, sink, requests) = as_429("/v1", &quota).await;
    assert_eq!(sink.events(), []);
    assert_eq!(requests, 1);
    assert_final(&refused, "insufficient_quota by type, as a 429");
    assert_eq!(refused.dispatch, Dispatch::Rejected);
}

/// The Codex backend's subscription limits, as the Codex CLI's fixtures shape them: a 429 whose
/// `error.type` is `usage_limit_reached` or `usage_not_included`, with no `error.code`. The Codex
/// CLI never retries either.
#[tokio::test]
async fn a_codex_backend_usage_limit_429_is_final() {
    for kind in ["usage_limit_reached", "usage_not_included"] {
        let limit = json!({
            "type": kind, "message": PROSE, "plan_type": "pro", "resets_at": 1_900_000_000
        });
        let (refused, sink, requests) = as_429("/backend-api/codex", &limit).await;
        assert_eq!(sink.events(), [], "{kind}");
        assert_eq!(requests, 1, "{kind}");
        assert_final(&refused, kind);
        assert_eq!(refused.dispatch, Dispatch::Rejected, "{kind}");
    }
}
