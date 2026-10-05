//! Retry classes the transport assigns, matching Harness `status_error`
//! (`harness-http/src/status.rs:27`-`34`, test `:43`) and its end-of-stream rule
//! (`harness-http/src/sse.rs:142`-`148`). The transport still sends exactly once: whether another
//! attempt is made is routing's decision, before any output is visible.

use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{Framing, HeaderMap, HttpClient, Limits, SseEvent, status_error};
use serde_json::json;
use std::time::{Duration, SystemTime};
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
