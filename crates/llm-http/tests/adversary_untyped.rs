//! Adversarial cases for the untyped-success reading in `post_sse`: a `2xx` with no
//! `content-type` is an event stream only when the request's `accept` asked for one, and a `2xx`
//! that names any other media type is still refused. Every answer is a local fixture.

use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{Framing, HeaderMap, HeaderValue, HttpClient, Limits, SseStream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const EVENT: &[u8] = b"data: {\"n\":1}\n\n";

/// Sends one request with `accept` and answers it with `head` and `body`.
async fn exchange(
    accept: Option<&'static str>,
    head: &'static [u8],
    body: &'static [u8],
) -> Result<SseStream, llm_core::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("a connection");
        let mut seen = Vec::new();
        let mut buffer = [0; 1024];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        let _ = socket.write_all(head).await;
        let _ = socket.write_all(body).await;
        let _ = socket.shutdown().await;
    });
    let mut headers = HeaderMap::new();
    if let Some(accept) = accept {
        headers.insert("accept", HeaderValue::from_static(accept));
    }
    let client = HttpClient::new(Limits::default()).expect("bounded transport");
    let outcome = client
        .post_sse(
            &url,
            headers,
            Vec::new(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await;
    server.await.expect("the fixture server");
    outcome
}

fn assert_refused_head(outcome: Result<SseStream, llm_core::Error>, case: &str) {
    let Err(error) = outcome else {
        panic!("{case}: a success that names no event stream was read as one");
    };
    assert_eq!(error.code, ErrorCode::Protocol, "{case}: {error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{case}: {error}");
    assert_eq!(
        error.message, "HTTP success response is not text/event-stream",
        "{case}: {error}"
    );
}

/// A `content-type` header that is present and empty names a media type that is not an event
/// stream; it is not the absent header the Codex reading covers.
#[tokio::test]
async fn an_empty_content_type_is_not_an_absent_one() {
    let outcome = exchange(
        Some("text/event-stream"),
        b"HTTP/1.1 200 OK\r\ncontent-type: \r\nconnection: close\r\n\r\n",
        EVENT,
    )
    .await;
    assert_refused_head(outcome, "empty content-type");
}

/// A media type that merely starts with `text/event-stream` is another media type.
#[tokio::test]
async fn a_media_type_that_only_starts_like_an_event_stream_is_refused() {
    let outcome = exchange(
        Some("text/event-stream"),
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-streamx\r\nconnection: close\r\n\r\n",
        EVENT,
    )
    .await;
    assert_refused_head(outcome, "text/event-streamx");
}

/// Media types are case-insensitive and parameters do not change them.
#[tokio::test]
async fn an_event_stream_in_another_case_with_parameters_is_read() {
    let mut stream = exchange(
        Some("text/event-stream"),
        b"HTTP/1.1 200 OK\r\ncontent-type: TEXT/Event-Stream; charset=utf-8\r\nconnection: close\r\n\r\n",
        EVENT,
    )
    .await
    .expect("an event stream in another case is an event stream");
    assert!(
        stream.next().await.expect("one event").is_some(),
        "the one event was not read"
    );
}

/// `accept: */*` admits an event stream but does not ask for one; an untyped success to it is
/// not read as an event stream.
#[tokio::test]
async fn an_untyped_success_to_a_wildcard_accept_is_refused() {
    let outcome = exchange(
        Some("*/*"),
        b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n",
        EVENT,
    )
    .await;
    assert_refused_head(outcome, "accept */*");
}

/// The first `content-type` decides; a later one cannot turn a JSON answer into a stream.
#[tokio::test]
async fn a_second_content_type_does_not_overrule_the_first() {
    let outcome = exchange(
        Some("text/event-stream"),
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        EVENT,
    )
    .await;
    assert_refused_head(outcome, "two content types");
}

/// The relaxation reaches every success code, not only `200`: an untyped `204` to a request that
/// asked for an event stream is read as an empty stream, and the protocol decoder above the
/// transport decides that no terminal event arrived.
#[tokio::test]
async fn an_untyped_no_content_success_is_an_empty_stream() {
    let mut stream = exchange(
        Some("text/event-stream"),
        b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n",
        b"",
    )
    .await
    .expect("an untyped 204 to an event-stream request is read as an empty stream");
    assert_eq!(stream.next().await.expect("a clean end"), None);
}
