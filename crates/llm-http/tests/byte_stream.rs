//! A streamed answer read as raw body chunks (`HttpClient::post_stream_until`), for a wire that
//! frames its own messages, and one JSON document fetched with `GET` (`HttpClient::get_json`).
//!
//! The byte stream keeps every rule of the event stream except the media type: the success must
//! name the media type the caller states, the body is bounded by `MAX_STREAM_BYTES`, each read by
//! the idle bound and all of them by the exchange's deadline and cancellation, and a failure
//! repeats. The JSON fetch keeps every rule of `post_json`.

use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{
    HeaderMap, HeaderValue, HttpClient, Limits, MAX_EXCHANGE_BYTES, MAX_STREAM_BYTES, ResponseTap,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

const EVENT_STREAM: &str = "application/vnd.amazon.eventstream";

/// A marker only an answer body carries; no diagnostic may quote it.
const ANSWER_MARKER: &str = "llm-fixture-byte-stream-answer-marker";

/// Reads one request head and its declared body; returns the head, lower-cased, and the body.
async fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
    };
    let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map_or(0, |value| value.trim().parse().unwrap());
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "the client closed inside its body");
        body.extend_from_slice(&buffer[..read]);
    }
    (head, body)
}

/// What the scripted server does after its answer's last part.
#[derive(Clone, Copy)]
enum Then {
    /// Closes the connection: a close-delimited body ends there.
    Close,
    /// Keeps the connection open until the client goes, at most ten seconds.
    Hold,
}

/// Serves one connection: `head`, then each of `parts` with a short pause between, then `then`.
/// Reports the request head and body it received.
async fn serve(
    head: String,
    parts: Vec<Vec<u8>>,
    then: Then,
) -> (String, JoinHandle<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/model/fixture/stream",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let received = read_request(&mut socket).await;
        socket.write_all(head.as_bytes()).await.unwrap();
        for part in parts {
            socket.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            // The client may already have gone, after a bound it enforces.
            if socket.write_all(&part).await.is_err() {
                return received;
            }
        }
        if let Then::Hold = then {
            let mut buffer = [0; 1024];
            let _ = tokio::time::timeout(Duration::from_secs(10), async {
                while socket.read(&mut buffer).await.is_ok_and(|read| read > 0) {}
            })
            .await;
        }
        received
    });
    (url, server)
}

/// A response head with `status`, `content-type` when given, `extra` lines and a
/// close-delimited body.
fn head(status: &str, content_type: Option<&str>, extra: &str) -> String {
    let content_type =
        content_type.map_or_else(String::new, |value| format!("content-type: {value}\r\n"));
    format!("HTTP/1.1 {status}\r\n{content_type}{extra}connection: close\r\n\r\n")
}

fn limits(idle: Duration) -> Limits {
    Limits {
        response_headers: Duration::from_secs(10),
        idle,
        total: Duration::from_secs(30),
    }
}

fn client() -> HttpClient {
    HttpClient::new(limits(Duration::from_secs(10))).unwrap()
}

fn stream_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("accept", HeaderValue::from_static(EVENT_STREAM));
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers
}

/// Reads the stream to its end, or to its first failure.
async fn drain(stream: &mut llm_http::ByteStream) -> (Vec<u8>, Result<(), llm_core::Error>) {
    let mut body = Vec::new();
    loop {
        match stream.next().await {
            Ok(Some(chunk)) => {
                assert!(!chunk.is_empty(), "a chunk is never empty");
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return (body, Ok(())),
            Err(error) => return (body, Err(error)),
        }
    }
}

/// Everything a tap was shown: the status line and the body bytes, in order.
#[derive(Default)]
struct Recorded {
    heads: Mutex<Vec<String>>,
    body: Mutex<Vec<u8>>,
}

impl ResponseTap for Recorded {
    fn head(&self, status_line: &str, _headers: &HeaderMap) {
        self.heads.lock().unwrap().push(status_line.to_owned());
    }
    fn chunk(&self, bytes: &[u8]) {
        self.body.lock().unwrap().extend_from_slice(bytes);
    }
}

/// A success naming the stated media type is read byte for byte, in as many chunks as the wire
/// delivers, then ends; the end repeats. The request went out once, as given, and a tap saw the
/// head and the same bytes.
#[tokio::test]
async fn a_success_naming_the_media_type_is_read_byte_for_byte_and_shown_to_a_tap() {
    let parts = vec![
        b"\x00\x00\x00\x10frame-one".to_vec(),
        Vec::new(),
        b"frame-two\xff\x00".to_vec(),
    ];
    let expected: Vec<u8> = parts.concat();
    let (url, server) = serve(
        head("200 OK", Some("application/vnd.amazon.eventstream"), ""),
        parts,
        Then::Close,
    )
    .await;
    let tap = Arc::new(Recorded::default());
    let client = client().with_response_tap(tap.clone());
    let mut stream = client
        .post_stream_until(
            &url,
            stream_headers(),
            b"{\"messages\":[]}".to_vec(),
            EVENT_STREAM,
            &Cancel::new(),
            None,
        )
        .await
        .unwrap();
    let (body, end) = drain(&mut stream).await;
    end.unwrap();
    assert_eq!(body, expected);
    assert!(stream.next().await.unwrap().is_none(), "the end repeats");
    let (request_head, request_body) = server.await.unwrap();
    assert!(request_head.starts_with("post /model/fixture/stream "));
    assert!(request_head.contains(&format!("accept: {EVENT_STREAM}")));
    assert_eq!(request_body, b"{\"messages\":[]}");
    assert_eq!(tap.heads.lock().unwrap().len(), 1);
    assert!(tap.heads.lock().unwrap()[0].contains("200"));
    assert_eq!(*tap.body.lock().unwrap(), expected);
}

/// The media type is compared without its parameters and in any case.
#[tokio::test]
async fn the_media_type_matches_without_parameters_in_any_case() {
    let (url, server) = serve(
        head(
            "200 OK",
            Some("Application/VND.Amazon.EventStream; charset=binary"),
            "",
        ),
        vec![b"x".to_vec()],
        Then::Close,
    )
    .await;
    let mut stream = client()
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &Cancel::new(),
            None,
        )
        .await
        .unwrap();
    let (body, end) = drain(&mut stream).await;
    end.unwrap();
    assert_eq!(body, b"x");
    server.await.unwrap();
}

/// A success naming another media type, or none, is refused as `protocol`, dispatch `accepted`,
/// whatever the request's `accept` asked for: only an event stream has the untyped reading.
#[tokio::test]
async fn a_success_of_another_media_type_or_none_is_refused_as_protocol() {
    for content_type in [Some("text/event-stream"), Some("application/json"), None] {
        let (url, server) = serve(
            head("200 OK", content_type, ""),
            vec![ANSWER_MARKER.as_bytes().to_vec()],
            Then::Close,
        )
        .await;
        let Err(error) = client()
            .post_stream_until(
                &url,
                stream_headers(),
                Vec::new(),
                EVENT_STREAM,
                &Cancel::new(),
                None,
            )
            .await
        else {
            panic!("{content_type:?} was accepted");
        };
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Protocol, Dispatch::Accepted),
            "{content_type:?}: {error:?}"
        );
        assert!(!format!("{error:?}").contains(ANSWER_MARKER));
        server.await.unwrap();
    }
}

fn fixture_quota(_headers: &HeaderMap, document: &Value) -> bool {
    document["error"] == "quota"
}

/// A failing status keeps the transport's status table: a 429 is a retriable rate limit unless
/// the client's quota rule reads it as an exhausted quota, which is final. A tap is shown the head
/// of a refused answer and never its body.
#[tokio::test]
async fn a_failing_status_keeps_the_status_table_and_the_quota_rule() {
    let cases = [
        (
            "{\"error\":\"slow down\"}",
            ErrorCode::RateLimited,
            Dispatch::Rejected,
            true,
        ),
        (
            "{\"error\":\"quota\"}",
            ErrorCode::Refused,
            Dispatch::Rejected,
            false,
        ),
    ];
    for (body, code, dispatch, retriable) in cases {
        let (url, server) = serve(
            head(
                "429 Too Many Requests",
                Some("application/json"),
                &format!("content-length: {}\r\n", body.len()),
            ),
            vec![body.as_bytes().to_vec()],
            Then::Close,
        )
        .await;
        let tap = Arc::new(Recorded::default());
        let client = client()
            .with_quota_rule(fixture_quota)
            .with_response_tap(tap.clone());
        let Err(error) = client
            .post_stream_until(
                &url,
                stream_headers(),
                Vec::new(),
                EVENT_STREAM,
                &Cancel::new(),
                None,
            )
            .await
        else {
            panic!("a 429 was accepted");
        };
        assert_eq!(
            (error.code, error.dispatch, error.retriable),
            (code, dispatch, retriable),
            "{body}: {error:?}"
        );
        assert_eq!(tap.heads.lock().unwrap().len(), 1);
        assert!(
            tap.body.lock().unwrap().is_empty(),
            "a refused body reached the tap"
        );
        server.await.unwrap();
    }
    let (url, server) = serve(
        head("400 Bad Request", Some("application/json"), ""),
        vec![ANSWER_MARKER.as_bytes().to_vec()],
        Then::Close,
    )
    .await;
    let Err(error) = client()
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &Cancel::new(),
            None,
        )
        .await
    else {
        panic!("a 400 was accepted");
    };
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Refused, Dispatch::Rejected, false)
    );
    assert!(!format!("{error:?}").contains(ANSWER_MARKER));
    server.await.unwrap();
}

/// A body cut short of its declared length is a retriable transport failure, dispatch
/// `accepted`, after the bytes that did arrive; the failure repeats on every later read.
#[tokio::test]
async fn a_cut_body_is_a_retriable_transport_failure_that_repeats() {
    let (url, server) = serve(
        head("200 OK", Some(EVENT_STREAM), "content-length: 64\r\n"),
        vec![b"sixteen bytes...".to_vec()],
        Then::Close,
    )
    .await;
    let mut stream = client()
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &Cancel::new(),
            None,
        )
        .await
        .unwrap();
    let (body, end) = drain(&mut stream).await;
    assert_eq!(body, b"sixteen bytes...");
    let error = end.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Transport, Dispatch::Accepted, true),
        "{error:?}"
    );
    assert_eq!(
        stream.next().await.unwrap_err(),
        error,
        "the failure repeats"
    );
    server.await.unwrap();
}

/// A body that falls silent for longer than the idle bound fails with `deadline`, dispatch
/// `accepted`, after what arrived before it.
#[tokio::test]
async fn a_silent_body_ends_at_the_idle_bound() {
    let (url, server) = serve(
        head("200 OK", Some(EVENT_STREAM), ""),
        vec![b"first".to_vec()],
        Then::Hold,
    )
    .await;
    let client = HttpClient::new(limits(Duration::from_millis(200))).unwrap();
    let mut stream = client
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &Cancel::new(),
            None,
        )
        .await
        .unwrap();
    let (body, end) = drain(&mut stream).await;
    assert_eq!(body, b"first");
    let error = end.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Deadline, Dispatch::Accepted)
    );
    drop(stream);
    server.await.unwrap();
}

/// The caller's own instant bounds every read: a body still open when it passes fails with
/// `deadline`, though the client's own bounds are far away.
#[tokio::test]
async fn the_callers_instant_bounds_every_read() {
    let (url, server) = serve(
        head("200 OK", Some(EVENT_STREAM), ""),
        vec![b"first".to_vec()],
        Then::Hold,
    )
    .await;
    let until = std::time::Instant::now() + Duration::from_millis(400);
    let mut stream = client()
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &Cancel::new(),
            Some(until),
        )
        .await
        .unwrap();
    let (_, end) = drain(&mut stream).await;
    let error = end.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Deadline, Dispatch::Accepted)
    );
    assert!(std::time::Instant::now() < until + Duration::from_secs(5));
    drop(stream);
    server.await.unwrap();
}

/// Cancellation ends a read that is waiting for the wire: `cancelled`, dispatch `accepted`.
#[tokio::test]
async fn cancellation_ends_a_waiting_read() {
    let (url, server) = serve(
        head("200 OK", Some(EVENT_STREAM), ""),
        vec![b"first".to_vec()],
        Then::Hold,
    )
    .await;
    let cancel = Cancel::new();
    let mut stream = client()
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &cancel,
            None,
        )
        .await
        .unwrap();
    let first = stream.next().await.unwrap().unwrap();
    assert!(b"first".starts_with(&first));
    let canceller = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        canceller.cancel();
    });
    let (_, end) = drain(&mut stream).await;
    let error = end.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Cancelled, Dispatch::Accepted)
    );
    drop(stream);
    server.await.unwrap();
}

/// A body past `MAX_STREAM_BYTES` is refused as `too-large`, dispatch `accepted`, and never
/// returned whole.
#[tokio::test]
async fn a_body_past_the_stream_bound_is_refused() {
    let parts = vec![
        vec![0x5a; MAX_STREAM_BYTES / 2],
        vec![0x5a; MAX_STREAM_BYTES / 2 + 1],
    ];
    let (url, server) = serve(head("200 OK", Some(EVENT_STREAM), ""), parts, Then::Close).await;
    let mut stream = client()
        .post_stream_until(
            &url,
            stream_headers(),
            Vec::new(),
            EVENT_STREAM,
            &Cancel::new(),
            None,
        )
        .await
        .unwrap();
    let (body, end) = drain(&mut stream).await;
    assert!(body.len() <= MAX_STREAM_BYTES);
    let error = end.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::TooLarge, Dispatch::Accepted)
    );
    drop(stream);
    server.await.unwrap();
}

/// `get_json` sends one `GET` with `accept: application/json`, no body and no content type, and
/// returns the document.
#[tokio::test]
async fn get_json_fetches_one_document() {
    let answer = json!({"AccessKeyId": "fixture", "n": 1}).to_string();
    let (url, server) = serve(
        head(
            "200 OK",
            Some("application/json"),
            &format!("content-length: {}\r\n", answer.len()),
        ),
        vec![answer.into_bytes()],
        Then::Close,
    )
    .await;
    let mut headers = HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_static("fixture-token"));
    let document: Value = client()
        .get_json(&url, headers, &Cancel::new())
        .await
        .unwrap();
    assert_eq!(document, json!({"AccessKeyId": "fixture", "n": 1}));
    let (request_head, request_body) = server.await.unwrap();
    assert!(request_head.starts_with("get /model/fixture/stream "));
    assert!(request_head.contains("accept: application/json"));
    assert!(request_head.contains("authorization: fixture-token"));
    assert!(!request_head.contains("content-type"));
    assert_eq!(request_body, [] as [u8; 0]);
}

/// Every refusal of `get_json` is final, a 503 included; an answer past `MAX_EXCHANGE_BYTES` is
/// `too-large`; neither body reaches a diagnostic, and a tap is shown nothing.
#[tokio::test]
async fn get_json_refusals_are_final_and_quote_nothing() {
    let tap = Arc::new(Recorded::default());
    let client = client().with_response_tap(tap.clone());
    let (url, server) = serve(
        head("503 Service Unavailable", Some("application/json"), ""),
        vec![ANSWER_MARKER.as_bytes().to_vec()],
        Then::Close,
    )
    .await;
    let error = client
        .get_json::<Value>(&url, HeaderMap::new(), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!((error.code, error.retriable), (ErrorCode::Transport, false));
    assert!(!format!("{error:?}").contains(ANSWER_MARKER));
    server.await.unwrap();
    let large = format!("\"{}\"", "a".repeat(MAX_EXCHANGE_BYTES));
    let (url, server) = serve(
        head("200 OK", Some("application/json"), ""),
        vec![large.into_bytes()],
        Then::Close,
    )
    .await;
    let error = client
        .get_json::<Value>(&url, HeaderMap::new(), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::TooLarge, Dispatch::Accepted, false)
    );
    server.await.unwrap();
    assert!(tap.heads.lock().unwrap().is_empty());
    assert!(tap.body.lock().unwrap().is_empty());
}

/// The member of the typed answer `get_json_decodes_into_the_type_asked_for` reads.
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
struct Typed {
    #[serde(rename = "AccessKeyId")]
    access_key_id: String,
    n: u32,
}

/// `get_json` decodes the answer into the type its caller names, other members ignored. JSON of
/// another shape is a final `protocol` failure that names the answer's length and quotes none of
/// it.
#[tokio::test]
async fn get_json_decodes_into_the_type_asked_for() {
    let answer = json!({"AccessKeyId": "fixture", "n": 1, "other": [true]}).to_string();
    let (url, server) = serve(
        head("200 OK", Some("application/json"), ""),
        vec![answer.into_bytes()],
        Then::Close,
    )
    .await;
    let typed: Typed = client()
        .get_json(&url, HeaderMap::new(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(
        typed,
        Typed {
            access_key_id: "fixture".to_owned(),
            n: 1
        }
    );
    server.await.unwrap();

    let answer = json!({"AccessKeyId": ANSWER_MARKER}).to_string();
    let length = answer.len();
    let (url, server) = serve(
        head("200 OK", Some("application/json"), ""),
        vec![answer.into_bytes()],
        Then::Close,
    )
    .await;
    let error = client()
        .get_json::<Typed>(&url, HeaderMap::new(), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Protocol, Dispatch::Accepted, false)
    );
    assert_eq!(
        error.message,
        format!("HTTP success answer of {length} byte(s) is not the JSON document expected")
    );
    server.await.unwrap();
}
