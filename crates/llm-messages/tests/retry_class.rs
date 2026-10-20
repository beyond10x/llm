//! The retry class of a Messages stream error (`docs/design.md` "Provider failures retried before
//! output", story:retry-classes-harness-retried). `overloaded_error` and `api_error` are the far
//! side's own temporary state, and Harness's wire retried both (`harness-messages/tests/
//! provider_emulated.rs:448`, "after 4 attempts" `:463`, at Harness `2fd7235b`): before any
//! output they may be attempted again. After any output, shown to the caller or not (a block
//! that opens empty), the turn has answered and the failure is final. Every other stream error
//! stays final. The same classes as an HTTP status before the stream opens are retriable too.
//! Local sockets only; no provider is contacted.
mod support;

use llm_core::{Cancel, Dispatch, Error, ErrorCode, Item, Model, TurnRequest, VecSink};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// The message header and a keep-alive: nothing answered yet.
const STARTED: &str = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\n";

/// A text block that opens empty: output the caller is not shown.
const SILENT_TEXT: &str = "event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n";

/// A thinking block that opens empty: output the caller is not shown.
const SILENT_THINKING: &str = "event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n";

/// An empty text block and its first delta: output the caller is shown.
const VISIBLE_TEXT: &str = "event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n";

/// The provider prose every fixture failure carries, which no diagnostic may repeat.
const PROSE: &str = "SECRET-UPSTREAM-TEXT";

/// The two stream errors retried before output: the type, the class it reads as and the fixed
/// diagnostic.
const TRANSIENT: [(&str, ErrorCode, &str); 2] = [
    (
        "overloaded_error",
        ErrorCode::Unavailable,
        "the Messages route reported that it is overloaded",
    ),
    (
        "api_error",
        ErrorCode::Transport,
        "the Messages route reported an internal failure",
    ),
];

fn error_event(kind: &str) -> String {
    let payload = serde_json::json!({"type": "error", "error": {"type": kind, "message": PROSE}});
    format!("event: error\ndata: {payload}\n\n")
}

fn client(url: &str) -> MessagesClient {
    let bound = Duration::from_secs(20);
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(Limits {
            response_headers: bound,
            idle: bound,
            total: Duration::from_secs(30),
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding")
}

/// Answers one request with `head` and `body`, closes, and returns the turn's refusal and what
/// the caller was shown.
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
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client(&url)
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the turn failed");
    server.await.expect("the fixture server");
    (error, sink)
}

async fn stream_turn(body: String) -> (Error, VecSink) {
    failed_turn(SSE_HEAD.to_vec(), body).await
}

#[tokio::test]
async fn overloaded_and_api_errors_before_output_are_retriable_without_provider_prose() {
    for (kind, class, message) in TRANSIENT {
        // Before the message header and after it: neither answers the turn.
        for opening in ["", STARTED] {
            let (error, sink) = stream_turn(format!("{opening}{}", error_event(kind))).await;
            assert_eq!(
                (error.code, error.dispatch),
                (class, Dispatch::Accepted),
                "`{kind}`"
            );
            assert!(error.may_retry(), "`{kind}` before output: {error:?}");
            assert_eq!(sink.events(), [], "`{kind}`");
            assert_eq!(error.message, message, "`{kind}`");
            assert!(!format!("{error:?}").contains(PROSE), "`{kind}`: {error:?}");
        }
    }
}

#[tokio::test]
async fn overloaded_and_api_errors_after_visible_or_silent_output_are_final() {
    for (kind, class, _) in TRANSIENT {
        for (output, visible) in [
            (SILENT_TEXT, false),
            (SILENT_THINKING, false),
            (VISIBLE_TEXT, true),
        ] {
            let (error, sink) =
                stream_turn(format!("{STARTED}{output}{}", error_event(kind))).await;
            assert_eq!(
                (error.code, error.dispatch),
                (class, Dispatch::Accepted),
                "`{kind}`"
            );
            assert_eq!(!sink.events().is_empty(), visible, "`{kind}`");
            assert!(
                !error.retriable && !error.may_retry(),
                "`{kind}`: an answered attempt cannot replay: {error:?}"
            );
            assert!(!format!("{error:?}").contains(PROSE), "`{kind}`: {error:?}");
        }
    }
}

/// Every other stream error is final before output too. A `rate_limit_error` may be a spend cap
/// that "keeps failing until access resumes" (Anthropic's errors page), and Harness's wire did
/// not retry it either; `billing_error` and an unknown or differently spelt type are refusals.
#[tokio::test]
async fn every_other_stream_error_stays_final_before_output() {
    for kind in [
        "rate_limit_error",
        "billing_error",
        "timeout_error",
        "authentication_error",
        "permission_error",
        "invalid_request_error",
        "request_too_large",
        "not_found_error",
        "unknown_failure",
        "OVERLOADED_ERROR",
    ] {
        let (error, sink) = stream_turn(format!("{STARTED}{}", error_event(kind))).await;
        assert_eq!(sink.events(), [], "`{kind}`");
        assert!(
            !error.retriable && !error.may_retry(),
            "`{kind}` is final: {error:?}"
        );
        assert!(!format!("{error:?}").contains(PROSE), "`{kind}`: {error:?}");
    }
}

/// The same two classes answered as an HTTP status before any stream: 529 and 500 are retriable
/// with dispatch `unknown`, and the error body is never read into a diagnostic.
#[tokio::test]
async fn the_same_classes_as_an_http_status_before_the_stream_are_retriable() {
    for (status, kind) in [
        ("529 Overloaded", "overloaded_error"),
        ("500 Internal Server Error", "api_error"),
    ] {
        let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": PROSE}})
            .to_string();
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let (error, sink) = failed_turn(head.into_bytes(), body).await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Transport, Dispatch::Unknown),
            "{status}"
        );
        assert!(error.may_retry(), "{status}: {error:?}");
        assert_eq!(sink.events(), [], "{status}");
        assert!(!format!("{error:?}").contains(PROSE), "{status}: {error:?}");
    }
}

/// A 429 answer before any stream, with `body` (sent as is) and the head fields in `fields`.
async fn answered_429(fields: &str, body: String) -> (Error, VecSink) {
    let head = format!(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\n{fields}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    failed_turn(head.into_bytes(), body).await
}

/// Anthropic's spend-cap error document, as the rate-limits page shows it, with `error` replaced.
fn anthropic_error(error: &serde_json::Value) -> String {
    serde_json::json!({"type": "error", "error": error, "request_id": "req_fixture"}).to_string()
}

/// story:quota-429-is-final. Anthropic's rate-limits page, "Reaching your spend cap": "On the
/// Messages API, `error.details.error_code` is `enforced_spend_limit_reached`. Use it to tell this
/// response apart from a rate limit." A 429 whose error says `rate_limit_error` with that code is
/// a final refusal with dispatch `rejected`, with or without a `retry-after` (whose hint is kept).
#[tokio::test]
async fn a_spend_cap_429_is_final_with_or_without_retry_after() {
    let spend_cap = anthropic_error(&serde_json::json!({
        "type": "rate_limit_error", "message": PROSE,
        "details": {"error_code": "enforced_spend_limit_reached"}
    }));
    for (fields, hint) in [("", None), ("retry-after: 7\r\n", Some(7_000))] {
        let (error, sink) = answered_429(fields, spend_cap.clone()).await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Refused, Dispatch::Rejected),
            "`{fields}`"
        );
        assert!(
            !error.retriable && !error.may_retry(),
            "`{fields}`: a spend cap keeps failing: {error:?}"
        );
        assert_eq!(error.retry_after_ms, hint, "`{fields}`");
        assert_eq!(sink.events(), [], "`{fields}`");
        assert!(
            !format!("{error:?} {error}").contains(PROSE),
            "`{fields}`: {error:?}"
        );
    }
}

/// Every other 429 keeps the rate-limit class, with or without a `retry-after`: a
/// `rate_limit_error` without the spend-cap code (so a rate limit answered without the header,
/// and the 429 Harness measured for a subscription token without the preamble, which carried no
/// rate-limit headers), the code under another type, in another spelling or outside `details`,
/// another type, no error at all, and a body that is not JSON.
#[tokio::test]
async fn any_other_429_stays_a_retriable_rate_limit() {
    let rate_limit =
        anthropic_error(&serde_json::json!({"type": "rate_limit_error", "message": PROSE}));
    for (fields, body, hint) in [
        ("", rate_limit.clone(), None),
        ("retry-after: 7\r\n", rate_limit, Some(7_000)),
        (
            "",
            anthropic_error(&serde_json::json!({"type": "rate_limit_error",
                "details": {"error_code": "rate_limit_exceeded"}})),
            None,
        ),
        (
            "",
            anthropic_error(&serde_json::json!({"type": "rate_limit_error",
                "details": {"error_code": "ENFORCED_SPEND_LIMIT_REACHED"}})),
            None,
        ),
        (
            "",
            anthropic_error(&serde_json::json!({"type": "rate_limit_error",
                "error_code": "enforced_spend_limit_reached"})),
            None,
        ),
        (
            "",
            anthropic_error(
                &serde_json::json!({"type": "overloaded_error", "message": PROSE,
                "details": {"error_code": "enforced_spend_limit_reached"}}),
            ),
            None,
        ),
        (
            "",
            anthropic_error(&serde_json::json!({"type": "RATE_LIMIT_ERROR",
                "details": {"error_code": "enforced_spend_limit_reached"}})),
            None,
        ),
        ("", "{}".to_owned(), None),
        ("", format!("{PROSE}, not a document"), None),
    ] {
        let (error, sink) = answered_429(fields, body.clone()).await;
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::RateLimited, Dispatch::Rejected),
            "`{fields}` {body}"
        );
        assert!(error.may_retry(), "`{fields}` {body}: {error:?}");
        assert_eq!(error.retry_after_ms, hint, "`{fields}` {body}");
        assert_eq!(sink.events(), [], "{body}");
        assert!(
            !format!("{error:?} {error}").contains(PROSE),
            "{body}: {error:?}"
        );
    }
}

/// Answers the first request with llm-gateway's refusal of a failure before output (HTTP 502,
/// code `upstream-failed`) carrying the head `fields`, and returns the turn's refusal, what the
/// caller was shown and how many requests arrived.
async fn relayed_turn(fields: &str) -> (Error, VecSink, usize) {
    let body =
        serde_json::json!({"error": {"code": "upstream-failed", "message": PROSE}}).to_string();
    let head = format!(
        "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\n{fields}content-length: {}\r\nconnection: close\r\n\r\n",
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
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        1 + usize::from(resent)
    });
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = client(&url)
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
            !format!("{error:?} {error}").contains(PROSE),
            "`{fields}`: {error:?}"
        );
    }
}

/// The start of the next event, cut before its payload ends.
const PARTIAL: &str = "event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\"";

/// A chunked stream head: a chunk the server stops writing is a body that failed in transit.
const CHUNKED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

/// The two ways a connection is cut mid-turn, each read by the transport as a retriable failure
/// with dispatch `accepted`.
#[derive(Clone, Copy, Debug)]
enum Cut {
    /// The body ends inside an event: `protocol`.
    InsideAnEvent,
    /// A chunked body ends inside a chunk: `transport`.
    InsideAChunk,
}

impl Cut {
    const ALL: [Self; 2] = [Self::InsideAnEvent, Self::InsideAChunk];

    const fn code(self) -> ErrorCode {
        match self {
            Self::InsideAnEvent => ErrorCode::Protocol,
            Self::InsideAChunk => ErrorCode::Transport,
        }
    }

    /// Serves `before` whole, then cuts the connection inside the next event.
    async fn turn(self, before: &str) -> (Error, VecSink) {
        match self {
            Self::InsideAnEvent => stream_turn(format!("{before}{PARTIAL}")).await,
            Self::InsideAChunk => {
                // A zero-length chunk would end the body cleanly, so an empty prefix sends none.
                let whole = if before.is_empty() {
                    String::new()
                } else {
                    format!("{:x}\r\n{before}\r\n", before.len())
                };
                // The last chunk promises 1024 bytes and carries a few.
                failed_turn(CHUNKED_HEAD.to_vec(), format!("{whole}400\r\n{PARTIAL}")).await
            }
        }
    }
}

/// H8 on this wire (story:messages-cut-after-silent-output-is-final): a cut before any payload
/// but `message_start` and `ping` keeps the transport's retry class, as on Responses
/// (`a_cut_after_only_lifecycle_events_stays_retriable`).
#[tokio::test]
async fn a_cut_before_any_block_stays_retriable() {
    for cut in Cut::ALL {
        for opening in ["", STARTED] {
            let (error, sink) = cut.turn(opening).await;
            assert_eq!(
                (error.code, error.dispatch),
                (cut.code(), Dispatch::Accepted),
                "{cut:?} after {opening:?}"
            );
            assert!(
                error.may_retry(),
                "{cut:?} after {opening:?}: nothing was answered yet: {error:?}"
            );
            assert_eq!(sink.events(), [], "{cut:?} after {opening:?}");
        }
    }
}

/// H8 and H18 on this wire: after any other payload the attempt has answered, whether the caller
/// was shown it or not, so a cut is final, as on Responses (`a_cut_after_an_output_item_is_final`).
/// Another attempt would answer, and bill, again. The failure keeps the attempt's evidence.
#[tokio::test]
async fn a_cut_after_silent_or_visible_output_is_final() {
    for cut in Cut::ALL {
        for (label, output, visible) in [
            ("an empty text block", SILENT_TEXT, false),
            ("an empty thinking block", SILENT_THINKING, false),
            ("a text delta", VISIBLE_TEXT, true),
        ] {
            let (error, sink) = cut.turn(&format!("{STARTED}{output}")).await;
            assert_eq!(
                (error.code, error.dispatch),
                (cut.code(), Dispatch::Accepted),
                "{cut:?} after {label}"
            );
            assert_eq!(!sink.events().is_empty(), visible, "{cut:?} after {label}");
            assert!(
                !error.retriable && !error.may_retry(),
                "{cut:?} after {label}: an answered attempt cannot replay: {error:?}"
            );
            assert!(error.observation.is_some(), "{cut:?} after {label}");
        }
    }
}
