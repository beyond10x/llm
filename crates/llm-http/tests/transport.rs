use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{Framing, HeaderMap, HeaderValue, HttpClient, Limits, SseEvent, retry_after};
use serde_json::json;
use std::time::{Duration, Instant, SystemTime};
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
    let failure = stream.next().await.unwrap_err();
    assert_eq!(failure.code, ErrorCode::Protocol);
    assert_eq!(failure.dispatch, Dispatch::Accepted);
    assert_eq!(stream.next().await.unwrap_err(), failure);
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

/// A caller that already holds an absolute instant — the start of a turn, a parent deadline —
/// bounds the whole exchange on it, headers and body alike, without restating the client's own
/// limits and letting the two drift apart.
///
/// Both halves are measured against a client whose own `total` is far longer than the instant, so
/// only the caller's instant can be what ends either wait.
#[tokio::test]
async fn a_caller_supplied_instant_bounds_the_response_headers_and_the_stream() {
    let patient = || {
        HttpClient::new(Limits {
            response_headers: Duration::from_secs(10),
            idle: Duration::from_secs(10),
            total: Duration::from_secs(10),
        })
        .unwrap()
    };
    let (headers_listener, headers_url) = listener().await;
    let headers_worker = tokio::spawn(async move {
        let (mut socket, _) = headers_listener.accept().await.unwrap();
        request(&mut socket).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        patient().post_sse_until(
            &headers_url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
            Some(Instant::now() + Duration::from_millis(300)),
        ),
    )
    .await
    .expect("the caller's instant did not end a request whose headers never arrived")
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::Deadline);
    headers_worker.abort();

    let (stream_listener, stream_url) = listener().await;
    let stream_worker = tokio::spawn(async move {
        let (mut socket, _) = stream_listener.accept().await.unwrap();
        request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket
            .write_all(b"data: {\"text\":\"prefix\"}\n\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let mut stream = patient()
        .post_sse_until(
            &stream_url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
            Some(Instant::now() + Duration::from_millis(300)),
        )
        .await
        .unwrap();
    assert!(stream.next().await.unwrap().is_some());
    let error = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("the caller's instant did not end a stream that stopped arriving")
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Deadline);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    stream_worker.abort();
}

/// The client's own `total` still ends the exchange when it is the shorter of the two, so a
/// caller cannot lengthen a bound by naming a later instant.
#[tokio::test]
async fn the_clients_own_total_still_ends_an_exchange_a_later_instant_would_not() {
    let (listener, url) = listener().await;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let client = HttpClient::new(Limits {
        response_headers: Duration::from_millis(50),
        ..Limits::default()
    })
    .unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        client.post_sse_until(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
            Some(Instant::now() + Duration::from_secs(600)),
        ),
    )
    .await
    .expect("a later caller instant lengthened the client's own bound")
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::Deadline);
    worker.abort();
}

/// Everything a tap was shown, in order.
#[derive(Default)]
struct Recorded(std::sync::Mutex<Vec<u8>>);

impl llm_http::ResponseTap for Recorded {
    fn head(&self, status_line: &str, headers: &HeaderMap) {
        let mut seen = self.0.lock().unwrap();
        seen.extend_from_slice(status_line.as_bytes());
        seen.extend_from_slice(b"\r\n");
        for (name, value) in headers {
            seen.extend_from_slice(name.as_str().as_bytes());
            seen.extend_from_slice(b": ");
            seen.extend_from_slice(value.as_bytes());
            seen.extend_from_slice(b"\r\n");
        }
        seen.extend_from_slice(b"\r\n");
    }
    fn chunk(&self, bytes: &[u8]) {
        self.0.lock().unwrap().extend_from_slice(bytes);
    }
}

/// A tap sees one streamed response as it arrived: status line, response headers, then the body
/// byte for byte. It is shown nothing the client sent, and a JSON exchange (the shape a credential
/// refresh takes) is never shown to it at all.
#[tokio::test]
async fn a_response_tap_sees_the_streamed_response_and_nothing_the_client_sent() {
    const SENT_TOKEN: &str = "request-credential-the-tap-never-sees";
    const BODY: &[u8] = b"event: one\ndata: {\"n\":1}\n\ndata: {\"n\":2}\n\n";
    let (listener, url) = listener().await;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nX-Fixture: seen\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        socket.write_all(&BODY[..9]).await.unwrap();
        socket.flush().await.unwrap();
        socket.write_all(&BODY[9..]).await.unwrap();
        socket.shutdown().await.unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"token\":\"t\"}")
            .await
            .unwrap();
        socket.shutdown().await.unwrap();
    });
    let tap = std::sync::Arc::new(Recorded::default());
    let client = client().with_response_tap(tap.clone());
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {SENT_TOKEN}")).unwrap(),
    );
    let mut stream = client
        .post_sse(
            &url,
            headers.clone(),
            format!("{{\"secret\":\"{SENT_TOKEN}\"}}").into_bytes(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        )
        .await
        .unwrap();
    while stream.next().await.unwrap().is_some() {}
    let streamed = tap.0.lock().unwrap().clone();
    client
        .post_json(&url, headers, b"{}".to_vec(), &Cancel::new())
        .await
        .unwrap();
    worker.await.unwrap();

    let seen = tap.0.lock().unwrap().clone();
    assert_eq!(seen, streamed, "a JSON exchange reached the tap");
    let text = String::from_utf8(seen).unwrap();
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(text.contains("\r\nx-fixture: seen\r\n"), "{text}");
    assert!(text.ends_with(std::str::from_utf8(BODY).unwrap()), "{text}");
    assert!(
        !text.to_ascii_lowercase().contains("authorization"),
        "{text}"
    );
    assert!(!text.contains(SENT_TOKEN), "{text}");
}

#[tokio::test]
async fn adversary_refused_response_tap_records_head_but_never_body() {
    for (status, media, expected) in [
        (
            "429 Too Many Requests",
            "text/event-stream",
            ErrorCode::RateLimited,
        ),
        ("200 OK", "application/json", ErrorCode::Protocol),
    ] {
        let (listener, url) = listener().await;
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket).await;
            socket.write_all(format!(
                "HTTP/1.1 {status}\r\nContent-Type: {media}\r\nRetry-After: 17\r\nConnection: close\r\n\r\nprivate-response-body"
            ).as_bytes()).await.unwrap();
        });
        let tap = std::sync::Arc::new(Recorded::default());
        let result = client()
            .with_response_tap(tap.clone())
            .post_sse(
                &url,
                HeaderMap::new(),
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &Cancel::new(),
            )
            .await;
        let error = match result {
            Ok(_) => panic!("a refused response became a stream"),
            Err(error) => error,
        };
        worker.await.unwrap();
        assert_eq!(error.code, expected);
        let seen = String::from_utf8(tap.0.lock().unwrap().clone()).unwrap();
        assert!(
            seen.starts_with(&format!("HTTP/1.1 {status}\r\n")),
            "{seen}"
        );
        assert!(seen.contains("\r\nretry-after: 17\r\n"), "{seen}");
        assert!(seen.ends_with("\r\n\r\n"), "{seen}");
        assert!(!seen.contains("private-response-body"), "{seen}");
    }
}
