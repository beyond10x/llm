//! One JSON document exchange (H33, Harness `harness-http/src/exchange.rs:64`, tests `:186`,
//! `:207`, `:227`, `:272`, `:280`): a JSON body out, one JSON document back, for a credential
//! exchange rather than a turn. It sends exactly once, follows no redirect, reads the answer up to
//! 64 KiB inclusive, quotes neither body in a diagnostic, and every refusal is final.

use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{HeaderMap, HttpClient, Limits, MAX_EXCHANGE_BYTES};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// A marker only the request body carries.
const REQUEST_MARKER: &str = "llm-fixture-json-request-private-marker";
/// A marker only a failing answer body carries.
const ANSWER_MARKER: &str = "llm-fixture-json-answer-private-marker";

/// What the scripted server received on its one connection.
struct Received {
    head: String,
    body: Vec<u8>,
}

/// Reads one request head and its declared body.
async fn read_request(socket: &mut TcpStream) -> Received {
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
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap_or(0);
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "the client closed inside its body");
        body.extend_from_slice(&buffer[..read]);
    }
    Received { head, body }
}

/// Serves one connection with `status`, `extra` header lines and `body`, then reports what it
/// received and how many connections arrived in total, a resend included.
async fn serve_once(
    status: &'static str,
    extra: String,
    body: Vec<u8>,
) -> (String, tokio::task::JoinHandle<(Received, usize)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let received = read_request(&mut socket).await;
        let head = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
        drop(socket);
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        (received, 1 + usize::from(resent))
    });
    (url, server)
}

fn client() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(30),
        idle: Duration::from_secs(30),
        total: Duration::from_secs(30),
    })
    .unwrap()
}

fn request_body() -> Vec<u8> {
    serde_json::to_vec(&json!({"grant_type": "fixture", "secret": REQUEST_MARKER})).unwrap()
}

async fn post(url: &str) -> Result<Value, llm_core::Error> {
    client()
        .post_json(url, HeaderMap::new(), request_body(), &Cancel::new())
        .await
}

fn assert_quotes_no_body(error: &llm_core::Error) {
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(
            !text.contains(REQUEST_MARKER),
            "request body quoted: {text}"
        );
        assert!(!text.contains(ANSWER_MARKER), "answer body quoted: {text}");
    }
}

/// H33: one POST carries the caller's JSON as `application/json`, and the JSON the server answered
/// comes back as a document.
#[tokio::test]
async fn a_json_document_exchange_posts_once_and_returns_the_answer() {
    let answer = json!({"issued": "fixture", "n": 1});
    let (url, server) = serve_once("200 OK", String::new(), answer.to_string().into_bytes()).await;
    let returned = post(&url).await.unwrap();
    let (received, connections) = server.await.unwrap();
    assert_eq!(returned, answer);
    assert_eq!(connections, 1);
    assert!(
        received.head.starts_with("POST /oauth/token HTTP/1.1"),
        "{}",
        received.head
    );
    assert!(
        received
            .head
            .lines()
            .any(|line| line.eq_ignore_ascii_case("content-type: application/json")),
        "{}",
        received.head
    );
    assert_eq!(received.body, request_body());
}

/// H33: every failing status keeps llm's code and dispatch evidence, is sent once, and is final:
/// a credential exchange is not a turn, so the status table's retry class is withdrawn.
#[tokio::test]
async fn every_refusal_of_a_json_exchange_is_final_and_sent_once() {
    let table: [(&'static str, ErrorCode, Dispatch); 9] = [
        ("400 Bad Request", ErrorCode::Refused, Dispatch::Rejected),
        (
            "401 Unauthorized",
            ErrorCode::Unauthorized,
            Dispatch::Rejected,
        ),
        ("403 Forbidden", ErrorCode::Unauthorized, Dispatch::Rejected),
        (
            "408 Request Timeout",
            ErrorCode::Transport,
            Dispatch::Unknown,
        ),
        (
            "429 Too Many Requests",
            ErrorCode::RateLimited,
            Dispatch::Rejected,
        ),
        (
            "500 Internal Server Error",
            ErrorCode::Transport,
            Dispatch::Unknown,
        ),
        (
            "503 Service Unavailable",
            ErrorCode::Transport,
            Dispatch::Unknown,
        ),
        ("529 Overloaded", ErrorCode::Transport, Dispatch::Unknown),
        ("409 Conflict", ErrorCode::Refused, Dispatch::Rejected),
    ];
    for (status, code, dispatch) in table {
        let body = format!(r#"{{"error":"{ANSWER_MARKER}"}}"#).into_bytes();
        let (url, server) = serve_once(status, "retry-after: 1\r\n".to_owned(), body).await;
        let error = post(&url).await.unwrap_err();
        let (_, connections) = server.await.unwrap();
        assert_eq!((error.code, error.dispatch), (code, dispatch), "{status}");
        assert!(!error.retriable, "{status} offered for retry: {error}");
        assert!(!error.may_retry(), "{status}");
        assert_eq!(connections, 1, "{status} was sent again");
        assert!(error.to_string().contains(&status[..3]), "{error}");
        assert_quotes_no_body(&error);
    }
}

/// H33: a redirect is a refusal and never a second request, so the body cannot cross origins.
#[tokio::test]
async fn a_json_exchange_redirect_is_refused_and_never_followed() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let location = format!(
        "location: http://{}/elsewhere\r\n",
        target.local_addr().unwrap()
    );
    let (url, server) = serve_once("307 Temporary Redirect", location, Vec::new()).await;
    let error = post(&url).await.unwrap_err();
    let (_, connections) = server.await.unwrap();
    assert_eq!(error.code, ErrorCode::Refused, "{error}");
    assert_eq!(error.dispatch, Dispatch::Unknown, "{error}");
    assert!(!error.retriable);
    assert_eq!(connections, 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err(),
        "the redirect target was contacted"
    );
}

/// H33: the answer bound is 64 KiB, inclusive; one byte more is `too-large` and final.
#[tokio::test]
async fn the_answer_bound_is_inclusive_at_64_kib_and_one_byte_more_is_too_large() {
    assert_eq!(MAX_EXCHANGE_BYTES, 64 * 1024);
    for total in [MAX_EXCHANGE_BYTES - 1, MAX_EXCHANGE_BYTES] {
        let body = format!("\"{}\"", "x".repeat(total - 2)).into_bytes();
        assert_eq!(body.len(), total);
        let (url, server) = serve_once("200 OK", String::new(), body).await;
        let value = post(&url).await.unwrap();
        server.await.unwrap();
        assert_eq!(value.as_str().unwrap().len(), total - 2, "{total}");
    }
    let body = format!(
        "\"{}é\"",
        "x".repeat(MAX_EXCHANGE_BYTES - 2 - 'é'.len_utf8())
    )
    .into_bytes();
    assert_eq!(body.len(), MAX_EXCHANGE_BYTES);
    let (url, server) = serve_once("200 OK", String::new(), body).await;
    let value = post(&url).await.unwrap();
    server.await.unwrap();
    assert!(value.as_str().unwrap().ends_with('é'));

    let body = vec![b'x'; MAX_EXCHANGE_BYTES + 1];
    let (url, server) = serve_once("200 OK", String::new(), body).await;
    let error = post(&url).await.unwrap_err();
    let (_, connections) = server.await.unwrap();
    assert_eq!(error.code, ErrorCode::TooLarge, "{error}");
    assert!(!error.retriable);
    assert_eq!(connections, 1);
}

/// H33: a success whose body is not JSON is a final `protocol` refusal with dispatch `accepted`,
/// and the body is not quoted.
#[tokio::test]
async fn a_success_that_is_not_json_is_a_final_protocol_refusal() {
    let body = format!("not json {ANSWER_MARKER}").into_bytes();
    let (url, server) = serve_once("200 OK", String::new(), body).await;
    let error = post(&url).await.unwrap_err();
    let (_, connections) = server.await.unwrap();
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert!(!error.retriable);
    assert_eq!(connections, 1);
    assert_quotes_no_body(&error);
}

/// H33: a connection nobody accepts is a transport failure that is final, and names no body.
#[tokio::test]
async fn a_refused_connection_is_a_final_transport_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    drop(listener);
    let error = post(&url).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Transport, "{error}");
    assert!(!error.retriable, "{error}");
    assert_quotes_no_body(&error);
}

/// H33: a cancelled exchange sends nothing, and the URL rules of `post_sse` hold.
#[tokio::test]
async fn a_cancelled_or_malformed_exchange_sends_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    let cancel = Cancel::new();
    cancel.cancel();
    let error = client()
        .post_json(&url, HeaderMap::new(), request_body(), &cancel)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    let embedded = url.replace("http://", "http://user:pass@");
    let error = post(&embedded).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "a refused exchange reached the server"
    );
}
