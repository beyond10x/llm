//! One form exchange: an `application/x-www-form-urlencoded` body out, the answer back whatever
//! its status, for a credential exchange whose failing answers name their class in the body (the
//! AWS Security Token Service query API). It sends exactly once, follows no redirect, shows a
//! response tap nothing, reads a success up to 64 KiB and a failing body up to 16 KiB, and quotes
//! neither body in a diagnostic.

use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{HeaderMap, HttpClient, Limits, MAX_EXCHANGE_BYTES, MAX_REFUSAL_BYTES, ResponseTap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// A marker only the request form carries.
const FORM_MARKER: &str = "llm-fixture-form-request-private-marker";
/// A marker only an answer body carries.
const ANSWER_MARKER: &str = "llm-fixture-form-answer-private-marker";

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
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let received = read_request(&mut socket).await;
        let head = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
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

fn form() -> Vec<u8> {
    format!("Action=Probe&Version=2011-06-15&Marker={FORM_MARKER}").into_bytes()
}

fn assert_quotes_no_body(error: &llm_core::Error) {
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains(FORM_MARKER), "form quoted: {text}");
        assert!(!text.contains(ANSWER_MARKER), "answer body quoted: {text}");
    }
}

/// Records whatever it is shown.
#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

impl ResponseTap for Recorder {
    fn head(&self, status_line: &str, _headers: &HeaderMap) {
        self.0.lock().unwrap().push(status_line.to_owned());
    }
    fn chunk(&self, bytes: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(bytes).into_owned());
    }
}

/// One POST carries the caller's form as `application/x-www-form-urlencoded`, and the success
/// comes back whole with its status; nothing reaches a response tap, and the answer's debug
/// form shows neither body.
#[tokio::test]
async fn a_form_is_posted_once_and_its_success_comes_back_whole() {
    let answer = format!("<Answer>{ANSWER_MARKER}</Answer>").into_bytes();
    let (url, server) = serve_once("200 OK", String::new(), answer.clone()).await;
    let tap = Arc::new(Recorder::default());
    let returned = client()
        .with_response_tap(tap.clone())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    let (received, connections) = server.await.unwrap();
    assert_eq!(connections, 1);
    assert!(
        received.head.starts_with("POST / HTTP/1.1"),
        "{}",
        received.head
    );
    assert!(
        received.head.lines().any(|line| line.eq_ignore_ascii_case(
            "content-type: application/x-www-form-urlencoded; charset=utf-8"
        )),
        "{}",
        received.head
    );
    assert_eq!(received.body, form());
    assert_eq!(returned.status(), 200);
    assert_eq!(returned.body(), Some(answer.as_slice()));
    assert!(tap.0.lock().unwrap().is_empty(), "a tap saw the exchange");
    let debug = format!("{returned:?}");
    assert!(!debug.contains(ANSWER_MARKER), "{debug}");
    assert!(!debug.contains(FORM_MARKER), "{debug}");
}

/// A failing status is an answer, not an error: its status, headers and body come back, so the
/// caller can read the class the body names. It is sent once and never again.
#[tokio::test]
async fn a_failing_status_comes_back_with_its_headers_and_body() {
    for status in [
        "400 Bad Request",
        "403 Forbidden",
        "503 Service Unavailable",
    ] {
        let body = format!("<ErrorResponse>{ANSWER_MARKER}</ErrorResponse>").into_bytes();
        let (url, server) = serve_once(status, "retry-after: 7\r\n".to_owned(), body.clone()).await;
        let returned = client()
            .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
            .await
            .unwrap();
        let (_, connections) = server.await.unwrap();
        assert_eq!(connections, 1, "{status} was sent again");
        assert_eq!(returned.status().to_string(), status[..3], "{status}");
        assert_eq!(
            returned
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
            Some("7"),
            "{status}"
        );
        assert_eq!(returned.body(), Some(body.as_slice()), "{status}");
    }
}

/// A failing body is read up to 16 KiB inclusive; one byte more is left out while the status
/// stays, so a long refusal never changes its class.
#[tokio::test]
async fn a_failing_body_past_the_refusal_bound_is_left_out_and_its_status_kept() {
    assert_eq!(MAX_REFUSAL_BYTES, 16 * 1024);
    let body = vec![b'x'; MAX_REFUSAL_BYTES];
    let (url, server) = serve_once("400 Bad Request", String::new(), body.clone()).await;
    let returned = client()
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(returned.body().map(<[u8]>::len), Some(MAX_REFUSAL_BYTES));

    let (url, server) = serve_once(
        "400 Bad Request",
        String::new(),
        vec![b'x'; MAX_REFUSAL_BYTES + 1],
    )
    .await;
    let returned = client()
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(returned.status(), 400);
    assert_eq!(returned.body(), None);
}

/// A success is read up to 64 KiB inclusive; one byte more is `too-large` and final.
#[tokio::test]
async fn a_success_past_the_exchange_bound_is_too_large() {
    let (url, server) = serve_once("200 OK", String::new(), vec![b'x'; MAX_EXCHANGE_BYTES]).await;
    let returned = client()
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(returned.body().map(<[u8]>::len), Some(MAX_EXCHANGE_BYTES));

    let (url, server) =
        serve_once("200 OK", String::new(), vec![b'x'; MAX_EXCHANGE_BYTES + 1]).await;
    let error = client()
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap_err();
    let (_, connections) = server.await.unwrap();
    assert_eq!(error.code, ErrorCode::TooLarge, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert!(!error.retriable);
    assert_eq!(connections, 1);
}

/// A redirect is an answer like any other: it is never followed, so the form never reaches the
/// redirect target.
#[tokio::test]
async fn a_redirect_is_an_answer_and_never_followed() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let location = format!(
        "location: http://{}/elsewhere\r\n",
        target.local_addr().unwrap()
    );
    let (url, server) = serve_once("307 Temporary Redirect", location, Vec::new()).await;
    let returned = client()
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    let (_, connections) = server.await.unwrap();
    assert_eq!(returned.status(), 307);
    assert_eq!(connections, 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err(),
        "the redirect target was contacted"
    );
}

/// A connection nobody accepts is a final transport failure; a cancelled or malformed exchange
/// sends nothing. No failure names the form.
#[tokio::test]
async fn a_refused_cancelled_or_malformed_exchange_is_final_and_quotes_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let cancel = Cancel::new();
    cancel.cancel();
    let error = client()
        .post_form(&url, HeaderMap::new(), form(), &cancel)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    let embedded = url.replace("http://", "http://user:pass@");
    let error = client()
        .post_form(&embedded, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_quotes_no_body(&error);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "a refused exchange reached the server"
    );
    drop(listener);
    let error = client()
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Transport, "{error}");
    assert!(!error.retriable, "{error}");
    assert_quotes_no_body(&error);
}
