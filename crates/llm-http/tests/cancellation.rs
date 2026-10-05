//! Harness parity H25 and H26: cancellation of `post_sse` before it sends, and while it waits
//! for response headers (`harness-http/src/transport.rs:223`, test `:637`; `:477`-`482`, test
//! `:731`). Local sockets only.
use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{Framing, HeaderMap, HttpClient, Limits};
use std::time::{Duration, Instant};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

/// Reads one request head and its declared body.
async fn request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 64 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse().unwrap());
            if bytes.len() >= end + 4 + length {
                return;
            }
        }
    }
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, url)
}

/// Long enough that only the cancellation can be what ends either wait.
fn patient() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(10),
    })
    .unwrap()
}

/// H25. The address answers, so a request that escaped would be accepted: what is under test is
/// that the check comes first, and that the refusal says nothing was sent.
#[tokio::test]
async fn a_cancelled_post_sse_never_sends() {
    let (listener, url) = listener().await;
    let cancel = Cancel::new();
    cancel.cancel();
    let started = Instant::now();
    let error = patient()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
        )
        .await
        .err()
        .expect("a cancelled call returns no stream");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(error.code, ErrorCode::Cancelled, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
    assert!(!error.retriable, "{error}");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "a cancelled post_sse opened a connection"
    );
}

/// H26. The server has the whole request and never answers; the cancel arrives while the client
/// waits for response headers. It ends the call at once, says the request may have run, and
/// closes the connection rather than leaving it to the header deadline.
#[tokio::test]
async fn a_cancel_while_waiting_for_response_headers_aborts_the_request() {
    let (listener, url) = listener().await;
    let (arrived, arrived_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        arrived.send(()).unwrap();
        let mut byte = [0];
        let closed = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
            .await
            .ok()
            .and_then(Result::ok);
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        (closed, resent)
    });
    let cancel = Cancel::new();
    let canceller = {
        let cancel = cancel.clone();
        async move {
            arrived_rx.await.unwrap();
            cancel.cancel();
            Instant::now()
        }
    };
    let client = patient();
    let (cancelled_at, result) = tokio::join!(
        canceller,
        client.post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
        )
    );
    let error = result.err().expect("a cancelled call returns no stream");
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(1),
        "the call outlived its cancel by {:?}",
        cancelled_at.elapsed()
    );
    assert_eq!(error.code, ErrorCode::Cancelled, "{error}");
    assert_eq!(error.dispatch, Dispatch::Unknown, "{error}");
    assert!(!error.retriable, "{error}");
    let (closed, resent) = server.await.unwrap();
    assert_eq!(closed, Some(0), "the socket stayed open after the cancel");
    assert!(!resent, "the cancelled request was sent again");
}
