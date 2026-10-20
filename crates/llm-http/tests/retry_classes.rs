//! Retry classes the transport assigns, matching Harness `status_error`
//! (`harness-http/src/status.rs:27`-`34`, test `:43`) and its end-of-stream rule
//! (`harness-http/src/sse.rs:142`-`148`). The transport still sends exactly once: whether another
//! attempt is made is routing's decision, before any output is visible.

use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use llm_http::{
    Framing, HeaderMap, HeaderValue, HttpClient, Limits, MAX_REFUSAL_BYTES, REFUSAL_WAIT,
    ResponseTap, SseEvent, status_error,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// Every status Harness maps, with llm's code, dispatch evidence and retry class.
#[test]
fn every_status_harness_maps_has_its_code_dispatch_and_retry_class() {
    let table: [(u16, ErrorCode, Dispatch, bool); 15] = [
        (400, ErrorCode::Refused, Dispatch::Rejected, false),
        (401, ErrorCode::Unauthorized, Dispatch::Rejected, false),
        (403, ErrorCode::Unauthorized, Dispatch::Rejected, false),
        (404, ErrorCode::Refused, Dispatch::Rejected, false),
        (408, ErrorCode::Transport, Dispatch::Unknown, true),
        (409, ErrorCode::Refused, Dispatch::Rejected, false),
        (422, ErrorCode::Refused, Dispatch::Rejected, false),
        (429, ErrorCode::RateLimited, Dispatch::Rejected, true),
        (500, ErrorCode::Transport, Dispatch::Unknown, true),
        (502, ErrorCode::Transport, Dispatch::Unknown, true),
        (503, ErrorCode::Transport, Dispatch::Unknown, true),
        (504, ErrorCode::Transport, Dispatch::Unknown, true),
        (529, ErrorCode::Transport, Dispatch::Unknown, true),
        (599, ErrorCode::Transport, Dispatch::Unknown, true),
        (307, ErrorCode::Refused, Dispatch::Unknown, false),
    ];
    for (status, code, dispatch, retriable) in table {
        let error = status_error(status, &HeaderMap::new(), SystemTime::now());
        assert_eq!(
            (error.code, error.dispatch, error.retriable),
            (code, dispatch, retriable),
            "status {status}"
        );
        assert_eq!(error.may_retry(), retriable, "status {status}");
    }
}

async fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
    }
}

/// Serves one connection with `head` then `chunks`, closes, and counts any second connection.
async fn serve_once(
    head: &'static str,
    chunks: Vec<&'static [u8]>,
) -> (String, tokio::task::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(head.as_bytes()).await.unwrap();
        for chunk in chunks {
            socket.write_all(chunk).await.unwrap();
            tokio::task::yield_now().await;
        }
        drop(socket);
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        1 + usize::from(resent)
    });
    (url, server)
}

const STREAM_HEAD: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

async fn open(url: &str) -> llm_http::SseStream {
    HttpClient::new(Limits::default())
        .unwrap()
        .post_sse(
            url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .unwrap()
}

/// H8: an end of stream inside the first event yields nothing and is retriable; the request was
/// answered, so dispatch stays `accepted`, and the transport itself sends nothing again.
#[tokio::test]
async fn a_stream_cut_inside_its_first_event_is_retriable_and_keeps_accepted_dispatch() {
    let (url, server) = serve_once(STREAM_HEAD, vec![b"data: {\"partial\":"]).await;
    let mut stream = open(&url).await;
    let failure = stream.next().await.unwrap_err();
    assert_eq!(failure.code, ErrorCode::Protocol);
    assert_eq!(failure.dispatch, Dispatch::Accepted);
    assert!(failure.retriable, "a cut stream is worth another attempt");
    assert!(failure.may_retry());
    assert_eq!(server.await.unwrap(), 1, "the transport resent the request");
}

/// The class names what was observed, not whether anything was shown: a cut after a whole event
/// is retriable too, and routing refuses the retry because the event became visible.
#[tokio::test]
async fn a_cut_after_a_whole_event_is_still_retriable() {
    let (url, server) = serve_once(
        STREAM_HEAD,
        vec![b"data: {\"text\":\"prefix\"}\n\n", b"data: {\"partial\":"],
    )
    .await;
    let mut stream = open(&url).await;
    assert_eq!(
        stream.next().await.unwrap(),
        Some(SseEvent::Payload {
            event: None,
            data: json!({"text": "prefix"})
        })
    );
    let failure = stream.next().await.unwrap_err();
    assert_eq!(
        (failure.code, failure.dispatch, failure.retriable),
        (ErrorCode::Protocol, Dispatch::Accepted, true)
    );
    assert_eq!(server.await.unwrap(), 1);
}

/// A response body that fails while streaming is a dropped connection, retriable like a cut.
#[tokio::test]
async fn a_body_that_fails_while_streaming_is_retriable() {
    let (url, server) = serve_once(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 4096\r\n\r\n",
        vec![b"data: {\"n\":1}\n\n"],
    )
    .await;
    let mut stream = open(&url).await;
    let mut failure = None;
    for _ in 0..4 {
        match stream.next().await {
            Ok(Some(_)) => {}
            Ok(None) => panic!("a body shorter than its declared length ended cleanly"),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    let failure = failure.expect("the short body was refused");
    assert_eq!(
        (failure.code, failure.dispatch, failure.retriable),
        (ErrorCode::Transport, Dispatch::Accepted, true)
    );
    assert_eq!(server.await.unwrap(), 1);
}

/// A malformed frame meets the identical request again: final, unlike a cut.
#[tokio::test]
async fn a_frame_that_is_not_json_is_final() {
    let (url, server) = serve_once(STREAM_HEAD, vec![b"data: not json\n\n"]).await;
    let mut stream = open(&url).await;
    let failure = stream.next().await.unwrap_err();
    assert_eq!(
        (failure.code, failure.dispatch, failure.retriable),
        (ErrorCode::Protocol, Dispatch::Accepted, false)
    );
    assert!(!failure.may_retry());
    assert_eq!(server.await.unwrap(), 1);
}

/// A request that never reached an answer (here: nothing listens) is a retriable transport
/// failure, as Harness's `WireError::transport` is.
#[tokio::test]
async fn a_refused_connection_is_retriable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    drop(listener);
    let failure = HttpClient::new(Limits::default())
        .unwrap()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .err()
        .expect("nothing listens");
    assert_eq!(
        (failure.code, failure.dispatch, failure.retriable),
        (ErrorCode::Transport, Dispatch::Unknown, true)
    );
}

/// Deadlines and cancellation are the caller's own bounds, never a reason to try again.
#[tokio::test]
async fn deadlines_and_cancellation_are_final() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let limits = Limits {
        response_headers: Duration::from_millis(50),
        ..Limits::default()
    };
    let deadline = HttpClient::new(limits)
        .unwrap()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .err()
        .expect("the server never answers");
    assert_eq!(
        (deadline.code, deadline.retriable),
        (ErrorCode::Deadline, false)
    );
    server.abort();

    let cancel = Cancel::new();
    cancel.cancel();
    let cancelled = HttpClient::new(Limits::default())
        .unwrap()
        .post_sse(
            "http://127.0.0.1:9/v1",
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
        )
        .await
        .err()
        .expect("a cancelled request is refused");
    assert_eq!(
        (cancelled.code, cancelled.retriable),
        (ErrorCode::Cancelled, false)
    );
}

// ---------------------------------------------------------------------------------------------
// A 429 that reports an exhausted quota (story:quota-429-is-final). The transport knows no
// provider's error document: a protocol client hands it a rule, and the transport reads a 429's
// body for that rule only, bounded in bytes and in time.
// ---------------------------------------------------------------------------------------------

/// The prose every fixture error body carries, which no diagnostic may repeat.
const PROSE: &str = "SECRET-UPSTREAM-TEXT";

/// A fixture provider's quota document.
const QUOTA_BODY: &str = r#"{"error":{"code":"quota","message":"SECRET-UPSTREAM-TEXT"}}"#;

/// The rule a protocol client would state for that provider.
fn fixture_quota(_: &HeaderMap, document: &Value) -> bool {
    document.pointer("/error/code").and_then(Value::as_str) == Some("quota")
}

/// A rule that calls every document it is shown a quota.
fn every_document(_: &HeaderMap, _: &Value) -> bool {
    true
}

/// Records what a tap is shown.
#[derive(Default)]
struct Tap {
    heads: Mutex<Vec<String>>,
    chunks: Mutex<Vec<u8>>,
}

impl ResponseTap for Tap {
    fn head(&self, status_line: &str, _: &HeaderMap) {
        self.heads.lock().unwrap().push(status_line.to_owned());
    }
    fn chunk(&self, bytes: &[u8]) {
        self.chunks.lock().unwrap().extend_from_slice(bytes);
    }
}

/// A refusal head with `fields` (each ending in CRLF) and the empty line.
fn refusal_head(status: &str, fields: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{fields}\r\n")
}

/// Answers one request with `head` and then `body`, holds the connection for `hold` (so a body
/// shorter than it declares never ends), and counts a second request arriving meanwhile.
async fn serve_refusal(
    head: String,
    body: Vec<u8>,
    hold: Duration,
) -> (String, tokio::task::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(head.as_bytes()).await.unwrap();
        // The client may stop reading as soon as it has decided.
        let _ = socket.write_all(&body).await;
        let resent = tokio::time::timeout(hold, listener.accept()).await.is_ok();
        drop(socket);
        1 + usize::from(resent)
    });
    (url, server)
}

/// Sends one request, returns its refusal and how long the caller was held.
async fn refused(client: &HttpClient, url: &str) -> (Error, Duration) {
    let started = Instant::now();
    let error = client
        .post_sse(
            url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .err()
        .expect("a refused status");
    (error, started.elapsed())
}

fn quota_client() -> HttpClient {
    HttpClient::new(Limits::default())
        .unwrap()
        .with_quota_rule(fixture_quota)
}

fn assert_rate_limited(error: &Error, case: &str) {
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::RateLimited, Dispatch::Rejected, true),
        "{case}: {error:?}"
    );
    assert!(error.may_retry(), "{case}: {error:?}");
    assert!(!format!("{error:?} {error}").contains(PROSE), "{case}");
}

/// A 429 whose body the client's rule reads as an exhausted quota is a final refusal with
/// dispatch `rejected`: the class the same quota has inside a stream. Nothing is sent again, no
/// prose reaches the diagnostic, and the tap is still shown the head alone.
#[tokio::test]
async fn a_429_the_quota_rule_recognises_is_a_final_refusal_and_the_tap_sees_the_head_only() {
    let head = refusal_head(
        "429 Too Many Requests",
        &format!("Content-Length: {}\r\n", QUOTA_BODY.len()),
    );
    let (url, server) = serve_refusal(head, QUOTA_BODY.into(), Duration::from_millis(100)).await;
    let tap = Arc::new(Tap::default());
    let client = quota_client().with_response_tap(tap.clone());
    let (error, _) = refused(&client, &url).await;
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Refused, Dispatch::Rejected, false),
        "{error:?}"
    );
    assert!(!error.may_retry(), "{error:?}");
    assert!(!format!("{error:?} {error}").contains(PROSE), "{error:?}");
    assert_eq!(server.await.unwrap(), 1, "the transport resent the request");
    assert_eq!(
        *tap.heads.lock().unwrap(),
        ["HTTP/1.1 429 Too Many Requests"]
    );
    assert!(
        tap.chunks.lock().unwrap().is_empty(),
        "the tap was shown a refused body"
    );
}

/// Every other 429 keeps its class and its retry hint: a client without a rule, a document the
/// rule does not recognise, and a body that is not JSON.
#[tokio::test]
async fn a_429_no_rule_recognises_keeps_its_rate_limit_class_and_hint() {
    let unruled = HttpClient::new(Limits::default()).unwrap();
    for (client, body) in [
        (&unruled, QUOTA_BODY),
        (
            &quota_client(),
            r#"{"error":{"code":"slow_down","message":"SECRET-UPSTREAM-TEXT"}}"#,
        ),
        (&quota_client(), "SECRET-UPSTREAM-TEXT, not a document"),
    ] {
        let head = refusal_head(
            "429 Too Many Requests",
            &format!("Retry-After: 7\r\nContent-Length: {}\r\n", body.len()),
        );
        let (url, server) = serve_refusal(head, body.into(), Duration::from_millis(100)).await;
        let (error, _) = refused(client, &url).await;
        assert_rate_limited(&error, body);
        assert_eq!(error.retry_after_ms, Some(7_000), "{body}");
        assert_eq!(server.await.unwrap(), 1, "{body}");
    }
}

/// The body is read up to [`MAX_REFUSAL_BYTES`] inclusive. A body declared larger is not read at
/// all and one found larger is not read further: either keeps the status class, even when it is
/// the quota document.
#[tokio::test]
async fn a_429_body_past_the_byte_bound_keeps_its_class() {
    let padded = |length: usize| {
        let mut body = QUOTA_BODY.as_bytes().to_vec();
        body.resize(length, b' ');
        body
    };
    // Declared far past the bound; the few bytes sent never complete it.
    let head = refusal_head("429 Too Many Requests", "Content-Length: 100000000\r\n");
    let (url, server) = serve_refusal(head, QUOTA_BODY.into(), Duration::from_secs(3)).await;
    let (error, waited) = refused(&quota_client(), &url).await;
    assert_rate_limited(&error, "declared past the bound");
    assert!(waited < Duration::from_secs(2), "held for {waited:?}");
    server.abort();

    // No length declared: the body ends when the connection closes.
    for (length, quota) in [(MAX_REFUSAL_BYTES, true), (MAX_REFUSAL_BYTES + 1, false)] {
        let head = refusal_head("429 Too Many Requests", "Connection: close\r\n");
        let (url, server) = serve_refusal(head, padded(length), Duration::ZERO).await;
        let (error, _) = refused(&quota_client(), &url).await;
        if quota {
            assert_eq!(
                (error.code, error.retriable),
                (ErrorCode::Refused, false),
                "{length} bytes: {error:?}"
            );
        } else {
            assert_rate_limited(&error, &format!("{length} bytes"));
        }
        assert_eq!(server.await.unwrap(), 1, "{length} bytes");
    }
}

/// A body that stalls holds the caller no longer than [`REFUSAL_WAIT`], nor past the caller's
/// own instant or a cancellation, and keeps the status class.
#[tokio::test]
async fn a_stalled_429_body_holds_the_caller_only_within_its_bounds_and_keeps_its_class() {
    // Declares more than it sends, then holds the connection.
    let stalled = || {
        refusal_head(
            "429 Too Many Requests",
            &format!("Content-Length: {}\r\n", QUOTA_BODY.len() + 64),
        )
    };
    let (url, server) = serve_refusal(stalled(), QUOTA_BODY.into(), Duration::from_secs(5)).await;
    let (error, held) = refused(&quota_client(), &url).await;
    assert_rate_limited(&error, "stalled");
    assert!(
        held >= REFUSAL_WAIT && held < REFUSAL_WAIT + Duration::from_secs(1),
        "held for {held:?}"
    );
    server.abort();

    let (url, server) = serve_refusal(stalled(), QUOTA_BODY.into(), Duration::from_secs(5)).await;
    let started = Instant::now();
    let error = quota_client()
        .post_sse_until(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
            Some(std::time::Instant::now() + Duration::from_millis(150)),
        )
        .await
        .err()
        .expect("a refused status");
    assert_rate_limited(&error, "the caller's instant");
    assert!(
        started.elapsed() < REFUSAL_WAIT,
        "held for {:?}",
        started.elapsed()
    );
    server.abort();

    let (url, server) = serve_refusal(stalled(), QUOTA_BODY.into(), Duration::from_secs(5)).await;
    let cancel = Cancel::new();
    let canceller = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancel.cancel();
        })
    };
    let started = Instant::now();
    let error = quota_client()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
        )
        .await
        .err()
        .expect("a refused status");
    assert_rate_limited(&error, "cancelled while reading");
    assert!(
        started.elapsed() < REFUSAL_WAIT,
        "held for {:?}",
        started.elapsed()
    );
    canceller.await.unwrap();
    server.abort();
}

/// Only a 429's body is read for the rule: with a rule that calls every document a quota, a 503
/// and a 400 keep their classes, and a 429 does not.
#[tokio::test]
async fn the_quota_rule_is_consulted_for_a_429_only() {
    let client = HttpClient::new(Limits::default())
        .unwrap()
        .with_quota_rule(every_document);
    for (status, code, dispatch, retriable) in [
        (
            "503 Service Unavailable",
            ErrorCode::Transport,
            Dispatch::Unknown,
            true,
        ),
        (
            "400 Bad Request",
            ErrorCode::Refused,
            Dispatch::Rejected,
            false,
        ),
        (
            "429 Too Many Requests",
            ErrorCode::Refused,
            Dispatch::Rejected,
            false,
        ),
    ] {
        let head = refusal_head(status, "Content-Length: 2\r\n");
        let (url, server) = serve_refusal(head, b"{}".to_vec(), Duration::from_millis(100)).await;
        let (error, _) = refused(&client, &url).await;
        assert_eq!(
            (error.code, error.dispatch, error.retriable),
            (code, dispatch, retriable),
            "{status}"
        );
        assert_eq!(server.await.unwrap(), 1, "{status}");
    }
}

// ---------------------------------------------------------------------------------------------
// A do-not-retry signal on an error status (story:relayed-failures-keep-their-retry-decision):
// `x-should-retry: false`, which the official OpenAI and Anthropic SDKs obey before any status
// rule (`docs/design.md`). It only narrows: it never makes a final status retriable.
// ---------------------------------------------------------------------------------------------

/// Headers carrying one `x-should-retry` field per value.
fn signalled(values: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for value in values {
        headers.append("x-should-retry", HeaderValue::from_str(value).unwrap());
    }
    headers
}

/// Every status the table marks retriable reads final under the signal, with the same code,
/// dispatch, message and retry hint.
#[test]
fn a_do_not_retry_signal_makes_every_retriable_status_final_and_keeps_the_rest() {
    let now = SystemTime::now();
    for status in [408, 429, 500, 502, 503, 504, 529, 599] {
        let plain = status_error(status, &HeaderMap::new(), now);
        assert!(
            plain.may_retry(),
            "status {status} is retriable unsignalled"
        );
        for value in ["false", " false ", "\tfalse"] {
            let mut headers = signalled(&[value]);
            headers.insert("retry-after", HeaderValue::from_static("7"));
            let error = status_error(status, &headers, now);
            assert_eq!(
                (error.code, error.dispatch, &error.message),
                (plain.code, plain.dispatch, &plain.message),
                "status {status}, `{value}`"
            );
            assert!(
                !error.retriable && !error.may_retry(),
                "status {status}, `{value}`: {error:?}"
            );
            assert_eq!(error.retry_after_ms, Some(7_000), "status {status}");
        }
    }
}

/// The signal never widens: `x-should-retry: true` leaves a final status final. And only one
/// field whose value is exactly `false`, surrounding whitespace aside, is the signal: another
/// spelling or a repeated field (combined, `false, false`) changes nothing.
#[test]
fn the_signal_only_narrows_and_is_one_exact_false() {
    let now = SystemTime::now();
    for status in [400, 401, 403, 404, 409, 422, 307] {
        let error = status_error(status, &signalled(&["true"]), now);
        assert!(
            !error.retriable && !error.may_retry(),
            "status {status}: {error:?}"
        );
    }
    for values in [
        &["true"][..],
        &["False"],
        &["FALSE"],
        &["0"],
        &["no"],
        &[""],
        &["falsey"],
        &["false, true"],
        &["false", "false"],
        &["false", "true"],
    ] {
        let error = status_error(502, &signalled(values), now);
        assert!(
            error.retriable && error.may_retry(),
            "{values:?}: {error:?}"
        );
    }
}

/// On the wire: llm-gateway's 502 before output carrying the signal reads final and is sent
/// once; the field name is matched in any case.
#[tokio::test]
async fn a_502_carrying_the_signal_on_the_wire_reads_final() {
    let body = r#"{"error":{"code":"upstream-failed","message":"SECRET-UPSTREAM-TEXT"}}"#;
    for (field, retriable) in [
        ("x-should-retry: false\r\n", false),
        ("X-Should-Retry: false\r\n", false),
        ("", true),
    ] {
        let head = refusal_head(
            "502 Bad Gateway",
            &format!("{field}Content-Length: {}\r\n", body.len()),
        );
        let (url, server) = serve_refusal(head, body.into(), Duration::from_millis(100)).await;
        let (error, _) = refused(&HttpClient::new(Limits::default()).unwrap(), &url).await;
        assert_eq!(
            (error.code, error.dispatch, error.retriable),
            (ErrorCode::Transport, Dispatch::Unknown, retriable),
            "`{field}`"
        );
        assert!(!format!("{error:?} {error}").contains(PROSE), "`{field}`");
        assert_eq!(server.await.unwrap(), 1, "`{field}`");
    }
}
