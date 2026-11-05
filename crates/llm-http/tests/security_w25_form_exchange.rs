//! Security review of `HttpClient::post_form`, wave 2026-11-04-w25: the transport guarantees the
//! unit's own cases do not measure. Both body bounds hold for an answer that declares no length;
//! a late head, a stalled body and a trickling body are cut at the client's deadlines; a stalled
//! failing body is left out within the refusal wait; the answer's `Debug` form shows no header.
//!
//! Every value here is a fixture marker, never a credential.

use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{
    HeaderMap, HttpClient, Limits, MAX_EXCHANGE_BYTES, MAX_REFUSAL_BYTES, REFUSAL_WAIT,
};
use std::fmt::Write as _;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A marker only a response header carries.
const HEADER_MARKER: &str = "llm-fixture-sec-w25-header-marker";

/// How the fixture writes its answer after reading the request.
enum Script {
    /// These bytes, then the connection closed.
    Close(Vec<u8>),
    /// These bytes, then the connection held open without another byte.
    Hold(Vec<u8>),
    /// This head, then one byte of its body every 100 ms.
    Trickle(Vec<u8>),
}

/// Reads one request head and its declared body.
async fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        let length = socket.read(&mut buffer).await.unwrap_or(0);
        if length == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..length]);
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let declared: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut read = bytes.len() - end - 4;
    while read < declared {
        let length = socket.read(&mut buffer).await.unwrap_or(0);
        if length == 0 {
            return;
        }
        read += length;
    }
}

/// Serves one connection by `script`; the task ends with the test.
async fn serve(script: Script) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        match script {
            Script::Close(bytes) => {
                let _ = socket.write_all(&bytes).await;
            }
            Script::Hold(bytes) => {
                let _ = socket.write_all(&bytes).await;
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            Script::Trickle(head) => {
                let _ = socket.write_all(&head).await;
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if socket.write_all(b"x").await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    (url, server)
}

/// `status` with `body` in chunked transfer coding, 4 KiB a chunk: no length is declared.
fn chunked(status: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ntransfer-encoding: chunked\r\n\
         connection: close\r\n\r\n"
    )
    .into_bytes();
    for piece in body.chunks(4096) {
        let mut size = String::new();
        let _ = write!(size, "{:x}\r\n", piece.len());
        bytes.extend_from_slice(size.as_bytes());
        bytes.extend_from_slice(piece);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"0\r\n\r\n");
    bytes
}

fn client(limits: Limits) -> HttpClient {
    HttpClient::new(limits).unwrap()
}

fn roomy() -> Limits {
    Limits {
        response_headers: Duration::from_secs(30),
        idle: Duration::from_secs(30),
        total: Duration::from_secs(30),
    }
}

fn form() -> Vec<u8> {
    b"Action=Probe&Version=2011-06-15".to_vec()
}

/// A success that declares no length is held to 64 KiB while it is read: exactly the bound comes
/// back whole, one byte more is `too-large`, `accepted` and final.
#[tokio::test]
async fn a_success_without_a_declared_length_is_bounded_while_it_is_read() {
    let (url, _server) = serve(Script::Close(chunked(
        "200 OK",
        &vec![b'x'; MAX_EXCHANGE_BYTES],
    )))
    .await;
    let answer = client(roomy())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(answer.body().map(<[u8]>::len), Some(MAX_EXCHANGE_BYTES));

    let (url, _server) = serve(Script::Close(chunked(
        "200 OK",
        &vec![b'x'; MAX_EXCHANGE_BYTES + 1],
    )))
    .await;
    let error = client(roomy())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TooLarge, "{error:?}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error:?}");
    assert!(!error.retriable, "{error:?}");
}

/// A failing body that declares no length is held to 16 KiB while it is read: exactly the bound
/// comes back, one byte more is left out and the status stays.
#[tokio::test]
async fn a_failing_body_without_a_declared_length_is_bounded_and_its_status_kept() {
    let (url, _server) = serve(Script::Close(chunked(
        "400 Bad Request",
        &vec![b'x'; MAX_REFUSAL_BYTES],
    )))
    .await;
    let answer = client(roomy())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(answer.status(), 400);
    assert_eq!(answer.body().map(<[u8]>::len), Some(MAX_REFUSAL_BYTES));

    let (url, _server) = serve(Script::Close(chunked(
        "400 Bad Request",
        &vec![b'x'; MAX_REFUSAL_BYTES + 1],
    )))
    .await;
    let answer = client(roomy())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(answer.status(), 400);
    assert_eq!(answer.body(), None);
}

/// A head that never comes, a success body that stalls and one that trickles under the idle
/// bound are each cut at the client's own deadline: `deadline`, final, well within five seconds.
#[tokio::test]
async fn a_late_head_a_stalled_body_and_a_trickling_body_are_cut_at_the_client_deadlines() {
    let late_head = Limits {
        response_headers: Duration::from_millis(300),
        ..roomy()
    };
    let idle = Limits {
        idle: Duration::from_millis(300),
        ..roomy()
    };
    let total = Limits {
        idle: Duration::from_secs(1),
        total: Duration::from_millis(800),
        ..roomy()
    };
    let declared = b"HTTP/1.1 200 OK\r\ncontent-length: 4096\r\n\r\n".to_vec();
    for (case, script, limits) in [
        ("head", Script::Hold(Vec::new()), late_head),
        ("stalled body", Script::Hold(declared.clone()), idle),
        ("trickling body", Script::Trickle(declared), total),
    ] {
        let (url, _server) = serve(script).await;
        let started = Instant::now();
        let error = client(limits)
            .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
            .await
            .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{case}: {:?}",
            started.elapsed()
        );
        assert_eq!(error.code, ErrorCode::Deadline, "{case}: {error:?}");
        assert!(!error.retriable, "{case}: {error:?}");
    }
}

/// A failing body that stalls is left out once the refusal wait passes, long before the
/// client's 30 s deadline, and the status still comes back.
#[tokio::test]
async fn a_stalled_failing_body_is_left_out_within_the_refusal_wait() {
    let (url, _server) = serve(Script::Hold(
        b"HTTP/1.1 400 Bad Request\r\ncontent-length: 4096\r\n\r\n<ErrorResponse>".to_vec(),
    ))
    .await;
    let started = Instant::now();
    let answer = client(roomy())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    assert!(
        started.elapsed() < REFUSAL_WAIT + Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(answer.status(), 400);
    assert_eq!(answer.body(), None);
}

/// The answer's `Debug` form names the status and the body's length, never a header value.
#[tokio::test]
async fn the_answer_debug_form_shows_no_header_value() {
    let (url, _server) = serve(Script::Close(
        format!(
            "HTTP/1.1 403 Forbidden\r\nx-fixture: {HEADER_MARKER}\r\nset-cookie: {HEADER_MARKER}\r\n\
             content-length: 2\r\nconnection: close\r\n\r\nno"
        )
        .into_bytes(),
    ))
    .await;
    let answer = client(roomy())
        .post_form(&url, HeaderMap::new(), form(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(answer.status(), 403);
    let debug = format!("{answer:?}");
    assert!(!debug.contains(HEADER_MARKER), "{debug}");
}
