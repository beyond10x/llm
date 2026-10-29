//! Adversary pass 1, wave 2026-10-29-w23, story:context-length-refusal-is-typed.
//!
//! What the unit's own cases do not say: the code in shapes other than the ones listed there, the
//! byte bound at exactly its edge, a 400 body that never ends or that the caller cancels under,
//! a do-not-retry header that says "retry", the code after output, and the cause relayed through
//! this crate's own ingress encoder. Local sockets only.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item,
    Model, Protocol, Provenance, RefusalCause, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits, MAX_REFUSAL_BYTES, REFUSAL_WAIT};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{IngressStream, ResponsesClient, decode_stream};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SECRET: &str = "SECRET-UPSTREAM-TEXT";

const CONTEXT_400: &str =
    "HTTP endpoint returned status 400: the request does not fit the model's context window";

const PLAIN_400: &str = "HTTP endpoint returned status 400";

const SSE_HEAD: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const CREATED: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.in_progress\n\
data: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n";

const ITEM_ADDED: &str = "event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n";

const TEXT_DELTA: &str = "event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hel\"}\n\n";

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

/// Answers one request with `head` and `body`, then holds the connection for `hold` before
/// closing it, and returns the turn's refusal, what the sink was shown and how long the caller
/// waited. `cancel_after`, when set, cancels the turn that long after it starts.
async fn turn_against(
    head: String,
    body: Vec<u8>,
    hold: Duration,
    cancel_after: Option<Duration>,
) -> (Error, VecSink, Duration) {
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
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(&body).await;
        let _ = socket.flush().await;
        tokio::time::sleep(hold).await;
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
    let cancel = Cancel::new();
    let canceller = cancel_after.map(|after| {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            cancel.cancel();
        })
    });
    let started = Instant::now();
    let error = client
        .turn(&request, &mut sink, &cancel)
        .await
        .expect_err("the turn failed");
    let waited = started.elapsed();
    if let Some(canceller) = canceller {
        canceller.abort();
    }
    server.abort();
    (error, sink, waited)
}

/// A 400 head with a JSON media type and `fields` (each ending in CRLF).
fn head_400(fields: &str) -> String {
    format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\n{fields}\r\n")
}

/// A 400 sending `body` whole, with its length declared, then closing.
async fn answered_400(body: Vec<u8>, fields: &str) -> (Error, VecSink) {
    let head = head_400(&format!(
        "Content-Length: {}\r\nConnection: close\r\n{fields}",
        body.len()
    ));
    let (error, sink, _) = turn_against(head, body, Duration::ZERO, None).await;
    (error, sink)
}

fn context_document() -> serde_json::Value {
    json!({"error": {
        "message": SECRET, "type": "invalid_request_error",
        "param": "input", "code": "context_length_exceeded"
    }})
}

fn assert_plain_400(error: &Error, case: &str) {
    assert_eq!(
        (error.code, error.dispatch, error.retriable, error.refusal),
        (ErrorCode::Refused, Dispatch::Rejected, false, None),
        "{case}: {error:?}"
    );
    assert_eq!(error.message, PLAIN_400, "{case}");
    assert!(!format!("{error:?} {error}").contains(SECRET), "{case}");
}

fn assert_context_400(error: &Error, case: &str) {
    assert_eq!(
        (error.code, error.dispatch, error.retriable, error.refusal),
        (
            ErrorCode::Refused,
            Dispatch::Rejected,
            false,
            Some(RefusalCause::ContextLengthExceeded)
        ),
        "{case}: {error:?}"
    );
    assert!(!error.may_retry(), "{case}");
    assert_eq!(error.message, CONTEXT_400, "{case}");
    assert!(!format!("{error:?} {error}").contains(SECRET), "{case}");
}

/// The code is read exactly: surrounding whitespace, another separator, a prefix or suffix, an
/// array, a number, an object, a doubly nested `error.error.code` and the code under `type` or
/// `param` all name no cause, and the 400 is exactly the plain refusal.
#[tokio::test]
async fn the_code_in_any_shape_but_the_exact_string_names_no_cause() {
    for code in [
        json!(" context_length_exceeded"),
        json!("context_length_exceeded "),
        json!("context_length_exceeded\n"),
        json!("context-length-exceeded"),
        json!("Context_Length_Exceeded"),
        json!("context_length_exceeded_v2"),
        json!("x_context_length_exceeded"),
        json!(["context_length_exceeded"]),
        json!({"code": "context_length_exceeded"}),
        json!(400),
        json!(true),
        json!(""),
    ] {
        let body = json!({"error": {"code": code, "message": SECRET}}).to_string();
        let (error, sink) = answered_400(body.into_bytes(), "").await;
        assert_plain_400(&error, &format!("code {code}"));
        assert_eq!(sink.events(), []);
    }
    for body in [
        json!({"error": {"error": {"code": "context_length_exceeded"}, "message": SECRET}}),
        json!({"error": {"param": "context_length_exceeded", "message": SECRET}}),
        json!({"errors": [{"code": "context_length_exceeded", "message": SECRET}]}),
        json!([{"error": {"code": "context_length_exceeded"}}]),
        json!("context_length_exceeded"),
    ] {
        let (error, _) = answered_400(body.to_string().into_bytes(), "").await;
        assert_plain_400(&error, &format!("{body}"));
    }
}

/// The byte bound is inclusive: the context document padded to exactly [`MAX_REFUSAL_BYTES`]
/// names its cause; one byte more, declared or only found, names none.
#[tokio::test]
async fn the_byte_bound_is_inclusive_at_its_edge() {
    let mut at_bound = context_document().to_string().into_bytes();
    at_bound.resize(MAX_REFUSAL_BYTES, b' ');
    let (error, _) = answered_400(at_bound.clone(), "").await;
    assert_context_400(&error, "exactly at the bound, declared");

    let mut past_bound = at_bound.clone();
    past_bound.push(b' ');
    let (error, _) = answered_400(past_bound.clone(), "").await;
    assert_plain_400(&error, "one byte past the bound, declared");

    // Undeclared length: the body ends when the connection closes.
    for (body, named) in [(at_bound, true), (past_bound, false)] {
        let (error, _, _) = turn_against(
            head_400("Connection: close\r\n"),
            body,
            Duration::ZERO,
            None,
        )
        .await;
        if named {
            assert_context_400(&error, "exactly at the bound, undeclared");
        } else {
            assert_plain_400(&error, "one byte past the bound, undeclared");
        }
    }
}

/// A 400 whose body declares more than it sends and never ends holds the caller for no longer
/// than [`REFUSAL_WAIT`], and is the plain refusal: the cause it would have named is not guessed
/// from a partial document.
#[tokio::test]
async fn a_400_whose_body_never_ends_is_the_plain_refusal_within_the_refusal_wait() {
    let body = context_document().to_string().into_bytes();
    let head = head_400(&format!("Content-Length: {}\r\n", body.len() + 64));
    let (error, sink, waited) = turn_against(head, body, Duration::from_secs(5), None).await;
    assert_plain_400(&error, "stalled");
    assert_eq!(sink.events(), []);
    assert!(
        waited >= REFUSAL_WAIT && waited < REFUSAL_WAIT + Duration::from_secs(1),
        "waited for {waited:?}"
    );
}

/// A cancellation while the transport waits for a 400's body ends the wait at once. The turn
/// ends as a final, non-retriable failure with no cause, whichever code it reports.
#[tokio::test]
async fn a_cancellation_while_a_400_body_is_awaited_ends_the_wait_at_once() {
    let body = context_document().to_string().into_bytes();
    let head = head_400(&format!("Content-Length: {}\r\n", body.len() + 64));
    let (error, _, waited) = turn_against(
        head,
        body,
        Duration::from_secs(5),
        Some(Duration::from_millis(150)),
    )
    .await;
    assert!(waited < REFUSAL_WAIT, "waited for {waited:?}");
    assert_eq!(error.refusal, None, "{error:?}");
    assert!(!error.may_retry(), "{error:?}");
    assert!(!format!("{error:?} {error}").contains(SECRET));
}

/// `x-should-retry: true` never makes a context-length 400 retriable, and `false` changes
/// nothing either: the cause is named in both.
#[tokio::test]
async fn a_should_retry_header_leaves_a_context_length_400_final_and_named() {
    for value in ["true", "false"] {
        let (error, _) = answered_400(
            context_document().to_string().into_bytes(),
            &format!("x-should-retry: {value}\r\n"),
        )
        .await;
        assert_context_400(&error, &format!("x-should-retry: {value}"));
    }
}

/// A context-length failure after visible or silent output is final, names its cause, and the
/// text already shown stays shown.
#[tokio::test]
async fn a_context_length_failure_after_output_is_final_and_names_its_cause() {
    let failed = format!(
        "data: {}\n\n",
        json!({"type": "response.failed", "response": {
            "id": "resp_1", "status": "failed",
            "error": {"code": "context_length_exceeded", "message": SECRET}
        }})
    );
    let error_event = format!(
        "data: {}\n\n",
        json!({"type": "error", "code": "context_length_exceeded", "message": SECRET})
    );
    for (case, preface) in [
        ("silent output", ITEM_ADDED.to_owned()),
        ("visible output", format!("{ITEM_ADDED}{TEXT_DELTA}")),
    ] {
        for terminal in [&failed, &error_event] {
            let (error, sink, _) = turn_against(
                SSE_HEAD.to_owned(),
                format!("{CREATED}{preface}{terminal}").into_bytes(),
                Duration::ZERO,
                None,
            )
            .await;
            assert_eq!(
                (error.code, error.dispatch, error.retriable, error.refusal),
                (
                    ErrorCode::Refused,
                    Dispatch::Accepted,
                    false,
                    Some(RefusalCause::ContextLengthExceeded)
                ),
                "{case}: {error:?}"
            );
            assert_eq!(
                error.message,
                "the provider refused this request: the request does not fit the model's context window",
                "{case}"
            );
            assert!(!format!("{error:?} {error}").contains(SECRET), "{case}");
            let shown = !sink.events().is_empty();
            assert_eq!(
                shown,
                case == "visible output",
                "{case}: {:?}",
                sink.events()
            );
        }
    }
}

/// A context-length refusal relayed through this crate's ingress encoder reads back, through
/// this crate's decoder, as the refusal it was: `refused`, and with its cause. `fail` writes the
/// wire code `refused` for every refusal, and the decoder names a cause only for
/// `context_length_exceeded`, so a caller behind a Responses relay (the gateway) is told a plain
/// refusal and loses the cause the upstream named.
#[test]
fn a_context_length_refusal_relayed_through_the_ingress_encoder_reads_back_with_its_cause() {
    let provenance = Provenance {
        protocol: Protocol::Responses,
        provider: id("my-lab"),
        account: id("local"),
        endpoint: id("local-vllm"),
        model: id("small"),
        binding_revision: id("rev-1"),
    };
    let decoding_binding = llm_responses::Binding::new(provenance, id("example/Small-Model"));
    let upstream = Error::new(ErrorCode::Refused, CONTEXT_400)
        .with_dispatch(Dispatch::Rejected)
        .with_refusal(RefusalCause::ContextLengthExceeded);
    let mut stream = IngressStream::new("resp_gateway_1", 1_772_000_000);
    let payloads = stream.fail(&upstream);
    let failed = payloads.last().expect("a terminal payload");
    assert_eq!(failed["type"], "response.failed");
    let read_back = decode_stream(&decoding_binding, &payloads)
        .result
        .expect_err("a failed response is a refusal");
    assert_eq!(read_back.code, ErrorCode::Refused, "{read_back:?}");
    assert_eq!(
        read_back.refusal,
        Some(RefusalCause::ContextLengthExceeded),
        "relayed as wire code {}: {read_back:?}",
        failed["response"]["error"]["code"]
    );
}
