//! The retry class of a Responses turn cut mid-stream. The client hands events to the caller as
//! they arrive, but a payload can produce output without a visible event (an opening item), so
//! routing cannot count everything the provider produced: the client decides too. A cut before
//! any output stays retriable; a cut after any output item or delta is
//! final (Harness `a_turn_that_had_already_answered_is_never_retried`,
//! `harness-responses/tests/provider_emulated.rs:521`). A provider failure the far side reports
//! as its own temporary state (`server_is_overloaded`, `server_error`, `rate_limit_exceeded`)
//! follows the same rule; every other code is final. Local sockets only.

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
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CREATED: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.in_progress\n\
data: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n";

const ITEM_ADDED: &str = "event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n";

/// Cut inside the next event, before any terminal object.
const CUT: &str = "event: response.output_text.delta\ndata: {\"type\":\"response.output";

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

/// Serves `body` after an event-stream head, closes, and returns the turn's refusal.
async fn cut_turn(body: String) -> (Error, VecSink) {
    failed_turn(SSE_HEAD.to_vec(), body).await
}

/// Answers one request with `head` and `body`, closes, and returns the turn's refusal.
async fn failed_turn(head: Vec<u8>, body: String) -> (Error, VecSink) {
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
        socket.write_all(&head).await.expect("head written");
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
        .expect_err("the turn failed before a terminal object");
    server.await.expect("the fixture server");
    (error, sink)
}

#[tokio::test]
async fn a_cut_after_only_lifecycle_events_stays_retriable() {
    let (error, sink) = cut_turn(format!("{CREATED}{CUT}")).await;
    assert_eq!(sink.events(), []);
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Protocol, Dispatch::Accepted)
    );
    assert!(error.may_retry(), "nothing was answered yet: {error:?}");
}

#[tokio::test]
async fn a_cut_after_an_output_item_is_final() {
    let (error, sink) = cut_turn(format!("{CREATED}{ITEM_ADDED}{CUT}")).await;
    assert_eq!(sink.events(), []);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(
        !error.retriable && !error.may_retry(),
        "an output item was produced, so the turn had answered: {error:?}"
    );
}

fn failure_event(code: &str, nested: bool) -> String {
    let error = serde_json::json!({"code": code, "message": "SECRET-UPSTREAM-TEXT"});
    let payload = if nested {
        serde_json::json!({"type": "response.failed", "response": {
            "id": "resp_1", "status": "failed", "error": error
        }})
    } else {
        serde_json::json!({"type": "error", "code": code, "message": "SECRET-UPSTREAM-TEXT"})
    };
    format!("data: {payload}\n\n")
}

#[tokio::test]
async fn overload_before_output_is_retryable_availability_without_provider_prose() {
    for nested in [false, true] {
        let (error, sink) = cut_turn(format!(
            "{CREATED}{}",
            failure_event("server_is_overloaded", nested)
        ))
        .await;
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(error.dispatch, Dispatch::Accepted);
        assert!(error.may_retry(), "{error:?}");
        assert_eq!(sink.events(), []);
        assert_eq!(error.message, "the provider is temporarily overloaded");
        let observation = error.observation.expect("serving binding retained");
        assert!(observation.usage.is_none());
        assert!(!format!("{observation:?}").contains("SECRET-UPSTREAM-TEXT"));
    }
}

#[tokio::test]
async fn overload_after_visible_or_silent_output_is_final() {
    let delta = "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"hello\"}\n\n";
    for (output, visible) in [(ITEM_ADDED, false), (delta, true)] {
        for nested in [false, true] {
            let (error, sink) = cut_turn(format!(
                "{CREATED}{output}{}",
                failure_event("server_is_overloaded", nested)
            ))
            .await;
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert_eq!(error.dispatch, Dispatch::Accepted);
            assert_eq!(!sink.events().is_empty(), visible);
            assert!(
                !error.may_retry(),
                "an answered attempt cannot replay: {error:?}"
            );
        }
    }
}

#[tokio::test]
async fn unknown_codes_and_actual_refusals_are_never_retryable() {
    for code in [
        "policy_violation",
        "unknown_failure",
        "SERVER_IS_OVERLOADED",
    ] {
        let (error, _) = cut_turn(failure_event(code, false)).await;
        assert_eq!(error.code, ErrorCode::Refused);
        assert!(!error.may_retry());
        assert_eq!(error.message, "the provider refused this request");
    }
}

/// The provider failures retried before output (`docs/design.md` "Provider failures retried
/// before output", story:retry-classes-harness-retried): the code, the class it reads as and the
/// fixed diagnostic. Harness's wire retried `server_error` and `rate_limit_exceeded`
/// (`harness-responses/tests/provider_emulated.rs:341`, "after 4 attempts" `:357`, at Harness
/// `2fd7235b`); `server_is_overloaded` is the overload rule above.
const TRANSIENT: [(&str, ErrorCode, &str); 3] = [
    (
        "server_is_overloaded",
        ErrorCode::Unavailable,
        "the provider is temporarily overloaded",
    ),
    (
        "server_error",
        ErrorCode::Unavailable,
        "the provider failed this response on its own account",
    ),
    (
        "rate_limit_exceeded",
        ErrorCode::RateLimited,
        "the provider asked for less traffic",
    ),
];

const DELTA: &str =
    "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"hello\"}\n\n";

#[tokio::test]
async fn a_transient_failure_before_output_is_retriable_without_provider_prose() {
    for (code, class, message) in TRANSIENT {
        for nested in [false, true] {
            let (error, sink) = cut_turn(format!("{CREATED}{}", failure_event(code, nested))).await;
            assert_eq!(
                (error.code, error.dispatch),
                (class, Dispatch::Accepted),
                "`{code}`"
            );
            assert!(error.may_retry(), "`{code}` before output: {error:?}");
            assert_eq!(sink.events(), [], "`{code}`");
            assert_eq!(error.message, message, "`{code}`");
            assert!(
                !format!("{error:?}").contains("SECRET-UPSTREAM-TEXT"),
                "`{code}`: {error:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_transient_failure_after_visible_or_silent_output_is_final() {
    for (code, class, _) in TRANSIENT {
        for (output, visible) in [(ITEM_ADDED, false), (DELTA, true)] {
            for nested in [false, true] {
                let (error, sink) =
                    cut_turn(format!("{CREATED}{output}{}", failure_event(code, nested))).await;
                assert_eq!(
                    (error.code, error.dispatch),
                    (class, Dispatch::Accepted),
                    "`{code}`"
                );
                assert_eq!(!sink.events().is_empty(), visible, "`{code}`");
                assert!(
                    !error.retriable && !error.may_retry(),
                    "`{code}`: an answered attempt cannot replay: {error:?}"
                );
                assert!(
                    !format!("{error:?}").contains("SECRET-UPSTREAM-TEXT"),
                    "`{code}`: {error:?}"
                );
            }
        }
    }
}

/// A 429 that means an exhausted quota or spend limit is not a rate limit. The `OpenAI` error
/// codes page: "Retrying billing, spend, or quota errors won’t restore API access." Each such
/// code is a refusal, final before any output.
#[tokio::test]
async fn an_exhausted_quota_or_spend_limit_stays_final_before_output() {
    for code in [
        "insufficient_quota",
        "credit_balance_exhausted",
        "organization_spend_limit_exceeded",
        "project_spend_limit_exceeded",
        "organization_usage_limit_exceeded",
    ] {
        for nested in [false, true] {
            let (error, sink) = cut_turn(format!("{CREATED}{}", failure_event(code, nested))).await;
            assert_eq!(error.code, ErrorCode::Refused, "`{code}`");
            assert_eq!(sink.events(), [], "`{code}`");
            assert!(
                !error.retriable && !error.may_retry(),
                "`{code}` is final: {error:?}"
            );
            assert_eq!(
                error.message, "the provider refused this request",
                "`{code}`"
            );
        }
    }
}

/// The same classes answered as an HTTP status before any stream: 500, 503 and 429 are
/// retriable by the transport's status table, and the error body is never read into a
/// diagnostic.
#[tokio::test]
async fn the_same_classes_as_an_http_status_before_the_stream_are_retriable() {
    for (status, code, class, dispatch) in [
        (
            "500 Internal Server Error",
            "server_error",
            ErrorCode::Transport,
            Dispatch::Unknown,
        ),
        (
            "503 Service Unavailable",
            "server_is_overloaded",
            ErrorCode::Transport,
            Dispatch::Unknown,
        ),
        (
            "429 Too Many Requests",
            "rate_limit_exceeded",
            ErrorCode::RateLimited,
            Dispatch::Rejected,
        ),
    ] {
        let body = serde_json::json!({"error": {"code": code, "message": "SECRET-UPSTREAM-TEXT"}})
            .to_string();
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let (error, sink) = failed_turn(head.into_bytes(), body).await;
        assert_eq!((error.code, error.dispatch), (class, dispatch), "{status}");
        assert!(error.may_retry(), "{status}: {error:?}");
        assert_eq!(sink.events(), [], "{status}");
        assert!(
            !format!("{error:?}").contains("SECRET-UPSTREAM-TEXT"),
            "{status}: {error:?}"
        );
    }
}

/// The billing, spend and quota codes `docs/design.md` lists, each an HTTP 429 entry on the
/// `OpenAI` error codes page.
const QUOTA: [&str; 5] = [
    "insufficient_quota",
    "credit_balance_exhausted",
    "organization_spend_limit_exceeded",
    "project_spend_limit_exceeded",
    "organization_usage_limit_exceeded",
];

/// A 429 answer before any stream, with `body` and a declared length.
async fn answered_429(body: &serde_json::Value) -> (Error, VecSink) {
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    failed_turn(head.into_bytes(), body).await
}

/// story:quota-429-is-final. The same quota answered as an HTTP 429 before any stream is final
/// too, a refusal with dispatch `rejected`, so routing ends the run as it does for the quota
/// inside a stream. Only the machine-readable `error.code` decides; the prose never reaches a
/// diagnostic.
#[tokio::test]
async fn an_exhausted_quota_answered_as_a_429_before_the_stream_is_final() {
    for code in QUOTA {
        let (error, sink) = answered_429(&serde_json::json!({"error": {
            "message": "SECRET-UPSTREAM-TEXT", "type": "insufficient_quota", "param": null, "code": code
        }}))
        .await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Refused, Dispatch::Rejected),
            "`{code}`"
        );
        assert!(
            !error.retriable && !error.may_retry(),
            "`{code}` is final: {error:?}"
        );
        assert_eq!(sink.events(), [], "`{code}`");
        assert!(
            !format!("{error:?} {error}").contains("SECRET-UPSTREAM-TEXT"),
            "`{code}`: {error:?}"
        );
    }
}

/// Only those codes, exactly spelt, as `error.code`: every other 429 stays a retriable rate
/// limit, a quota named only by `error.type` or outside `error` included.
#[tokio::test]
async fn any_other_429_stays_a_retriable_rate_limit() {
    for body in [
        serde_json::json!({"error": {"code": "rate_limit_exceeded", "type": "insufficient_quota", "message": "SECRET-UPSTREAM-TEXT"}}),
        serde_json::json!({"error": {"code": null, "type": "insufficient_quota"}}),
        serde_json::json!({"error": {"code": "INSUFFICIENT_QUOTA"}}),
        serde_json::json!({"code": "insufficient_quota"}),
        serde_json::json!({}),
    ] {
        let (error, sink) = answered_429(&body).await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::RateLimited, Dispatch::Rejected),
            "{body}"
        );
        assert!(error.may_retry(), "{body}: {error:?}");
        assert_eq!(sink.events(), [], "{body}");
        assert!(
            !format!("{error:?} {error}").contains("SECRET-UPSTREAM-TEXT"),
            "{body}: {error:?}"
        );
    }
}

/// llm-gateway's refusal of a failure before output: HTTP 502, code `upstream-failed`.
const GATEWAY_502_BODY: &str =
    r#"{"error":{"code":"upstream-failed","message":"SECRET-UPSTREAM-TEXT"}}"#;

/// Answers the first request with llm-gateway's 502 carrying the head `fields`, and returns the
/// turn's refusal, what the caller was shown and how many requests arrived.
async fn relayed_turn(fields: &str) -> (Error, VecSink, usize) {
    let head = format!(
        "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\n{fields}content-length: {}\r\nconnection: close\r\n\r\n",
        GATEWAY_502_BODY.len()
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
            .write_all(GATEWAY_502_BODY.as_bytes())
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
        .expect_err("a 502 is a refusal");
    let requests = server.await.expect("the fixture server");
    (error, sink, requests)
}

/// story:relayed-failures-keep-their-retry-decision. A gateway that spent its retries on a
/// failure before output says so with `x-should-retry: false` on its 502: the client reads it
/// final and sends once, so routing does not call the gateway again. The same 502 without the
/// signal stays a retriable `transport`, dispatch `unknown`.
#[tokio::test]
async fn a_gateway_502_that_says_do_not_retry_is_final_and_without_it_is_retriable() {
    for (fields, retriable) in [("x-should-retry: false\r\n", false), ("", true)] {
        let (error, sink, requests) = relayed_turn(fields).await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Transport, Dispatch::Unknown),
            "`{fields}`"
        );
        assert_eq!(error.may_retry(), retriable, "`{fields}`: {error:?}");
        assert_eq!(requests, 1, "`{fields}`: the client sent the turn again");
        assert_eq!(sink.events(), [], "`{fields}`");
        assert!(
            !format!("{error:?} {error}").contains("SECRET-UPSTREAM-TEXT"),
            "`{fields}`: {error:?}"
        );
    }
}
