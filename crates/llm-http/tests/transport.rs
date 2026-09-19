use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{Framing, HeaderMap, HeaderValue, HttpClient, Limits, SseEvent, retry_after};
use serde_json::json;
use std::time::{Duration, SystemTime};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

async fn request(socket: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0);
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 64 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let content_length = header
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .unwrap()
                .trim()
                .parse::<usize>()
                .unwrap();
            if bytes.len() >= end + 4 + content_length {
                return bytes;
            }
        }
    }
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/arbitrary/prefix", listener.local_addr().unwrap());
    (listener, url)
}
fn client() -> HttpClient {
    HttpClient::new(Limits::default()).unwrap()
}

#[tokio::test]
async fn fragmented_response_preserves_stream_prefix_and_refuses_truncation_without_retry() {
    let (listener, url) = listener().await;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let captured = request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        for chunk in [
            b"data: {\"text\":\"prefix\"}\n\n".as_slice(),
            b"data: {\"partial\":",
        ] {
            socket.write_all(chunk).await.unwrap();
            tokio::task::yield_now().await;
        }
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "transport retried"
        );
        captured
    });
    let mut stream = client()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        stream.next().await.unwrap(),
        Some(SseEvent::Payload {
            event: None,
            data: json!({"text":"prefix"})
        })
    );
    assert_eq!(stream.next().await.unwrap_err().code, ErrorCode::Protocol);
    assert_eq!(stream.next().await.unwrap_err().code, ErrorCode::Protocol);
    let captured = worker.await.unwrap();
    assert!(captured.starts_with(b"POST /arbitrary/prefix HTTP/1.1"));
}

#[tokio::test]
async fn redirect_never_sends_credentials_to_a_second_endpoint() {
    let (destination, destination_url) = listener().await;
    let (origin, origin_url) = listener().await;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = origin.accept().await.unwrap();
        request(&mut socket).await;
        socket.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {destination_url}\r\nContent-Length: 0\r\n\r\n").as_bytes()).await.unwrap();
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer test-credential"),
    );
    let result = client()
        .post_sse(
            &origin_url,
            headers,
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await;
    let error = result.err().unwrap();
    assert_eq!(error.code, ErrorCode::Refused);
    assert!(!format!("{error:?}").contains("test-credential"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), destination.accept())
            .await
            .is_err()
    );
    worker.await.unwrap();
}

#[tokio::test]
async fn cancellation_closes_a_silent_stream_even_when_the_stream_value_is_retained() {
    let (listener, url) = listener().await;
    let (read_started, read_started_rx) = oneshot::channel();
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        read_started.send(()).unwrap();
        let mut bytes = [0; 8];
        tokio::time::timeout(Duration::from_secs(2), socket.read(&mut bytes))
            .await
            .unwrap()
            .unwrap()
    });
    let cancel = Cancel::new();
    let mut stream = client()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
        )
        .await
        .unwrap();
    read_started_rx.await.unwrap();
    let ((), result) = tokio::join!(
        async {
            tokio::task::yield_now().await;
            cancel.cancel();
        },
        stream.next()
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(worker.await.unwrap(), 0);
}

#[tokio::test]
async fn headers_deadline_records_ambiguous_dispatch_and_cancels_pending_request() {
    let (listener, url) = listener().await;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let client = HttpClient::new(Limits {
        response_headers: Duration::from_millis(50),
        ..Limits::default()
    })
    .unwrap();
    let error = client
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::Unknown);
    worker.await.unwrap();
}

#[tokio::test]
async fn status_retry_hint_does_not_retry_or_expose_the_untrusted_error_body() {
    let (listener, url) = listener().await;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 17\r\nContent-Length: 20\r\nConnection: close\r\n\r\nprivate-echo-payload!").await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let error = client()
        .post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(error.dispatch, Dispatch::Rejected);
    assert_eq!(error.retry_after_ms, Some(17_000));
    assert!(!format!("{error:?}").contains("private-echo"));
    worker.await.unwrap();
}

#[test]
fn retry_dates_and_extreme_delays_are_interpreted_without_early_retry() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut headers = HeaderMap::new();
    headers.insert(
        "retry-after",
        HeaderValue::from_str(&httpdate::fmt_http_date(now + Duration::from_secs(23))).unwrap(),
    );
    assert_eq!(retry_after(&headers, now), Some(Duration::from_secs(23)));
    headers.insert(
        "retry-after",
        HeaderValue::from_static("999999999999999999999999999"),
    );
    assert_eq!(
        retry_after(&headers, now),
        Some(Duration::from_secs(u64::MAX))
    );
    headers.insert("retry-after", HeaderValue::from_static("not a date"));
    assert_eq!(retry_after(&headers, now), None);
}

#[tokio::test]
async fn idle_and_total_deadlines_bound_silence_and_continuous_keepalives() {
    for keepalives in [false, true] {
        let (listener, url) = listener().await;
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            if keepalives {
                loop {
                    if socket.write_all(b":keepalive\n\n").await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            } else {
                let mut byte = [0];
                assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
            }
        });
        let limits = if keepalives {
            Limits {
                total: Duration::from_millis(80),
                idle: Duration::from_secs(1),
                ..Limits::default()
            }
        } else {
            Limits {
                idle: Duration::from_millis(30),
                ..Limits::default()
            }
        };
        let client = HttpClient::new(limits).unwrap();
        let mut stream = client
            .post_sse(
                &url,
                HeaderMap::new(),
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &Cancel::new(),
            )
            .await
            .unwrap();
        let error = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Deadline);
        assert_eq!(error.dispatch, Dispatch::Accepted);
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
    }
}
