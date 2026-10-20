//! Adversary, wave 2026-10-20-w10, unit http, pass 1. Probes of the 429 body read
//! (story:quota-429-is-final) and the do-not-retry signal on the wire
//! (story:relayed-failures-keep-their-retry-decision) that the unit's own tests do not drive:
//! a trickling body, a body declared at exactly `MAX_REFUSAL_BYTES`, chunked transfer coding, a
//! gzip content coding the transport never asked for, and repeated or contradicting signal
//! fields. Local sockets only.

use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use llm_http::{Framing, HeaderMap, HttpClient, Limits, MAX_REFUSAL_BYTES, REFUSAL_WAIT};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const PROSE: &str = "SECRET-UPSTREAM-TEXT";
const QUOTA_BODY: &str = r#"{"error":{"code":"quota","message":"SECRET-UPSTREAM-TEXT"}}"#;

fn fixture_quota(_: &HeaderMap, document: &Value) -> bool {
    document.pointer("/error/code").and_then(Value::as_str) == Some("quota")
}

fn quota_client() -> HttpClient {
    HttpClient::new(Limits::default())
        .unwrap()
        .with_quota_rule(fixture_quota)
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

/// Answers one request with `head`, then each of `parts` after `gap`, then closes; counts a
/// second request arriving within 100 ms after that.
async fn serve(
    head: String,
    parts: Vec<Vec<u8>>,
    gap: Duration,
) -> (String, tokio::task::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(head.as_bytes()).await.unwrap();
        for part in parts {
            if !gap.is_zero() {
                tokio::time::sleep(gap).await;
            }
            if socket.write_all(&part).await.is_err() {
                break;
            }
        }
        let _ = socket.shutdown().await;
        drop(socket);
        let resent = tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok();
        1 + usize::from(resent)
    });
    (url, server)
}

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

fn head(status: &str, fields: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{fields}\r\n")
}

fn assert_quota(error: &Error, case: &str) {
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::Refused, Dispatch::Rejected, false),
        "{case}: {error:?}"
    );
    assert!(!format!("{error:?} {error}").contains(PROSE), "{case}");
}

fn assert_rate_limited(error: &Error, case: &str) {
    assert_eq!(
        (error.code, error.dispatch, error.retriable),
        (ErrorCode::RateLimited, Dispatch::Rejected, true),
        "{case}: {error:?}"
    );
    assert!(!format!("{error:?} {error}").contains(PROSE), "{case}");
}

/// One byte every 900 ms: each read is inside any per-read bound, the whole is not inside
/// `REFUSAL_WAIT`. The caller is held about `REFUSAL_WAIT`, not 900 ms per byte.
#[tokio::test]
async fn a_429_body_that_trickles_is_cut_at_the_refusal_wait() {
    let parts = QUOTA_BODY.bytes().map(|byte| vec![byte]).collect();
    let (url, server) = serve(
        head(
            "429 Too Many Requests",
            &format!("Content-Length: {}\r\n", QUOTA_BODY.len()),
        ),
        parts,
        Duration::from_millis(900),
    )
    .await;
    let (error, held) = refused(&quota_client(), &url).await;
    assert_rate_limited(&error, "trickle");
    assert!(
        held >= REFUSAL_WAIT && held < REFUSAL_WAIT + Duration::from_millis(500),
        "held for {held:?}"
    );
    // The documented bound is one second (`docs/design.md`, `spec/domains/transport.yaml`), stated
    // as a literal so a longer `REFUSAL_WAIT` cannot pass by moving the bound with it.
    assert!(
        held < Duration::from_millis(1_500),
        "a 429 body held the caller {held:?}, past the documented 1 s"
    );
    server.abort();
}

/// "16 KiB, inclusive" with a declared length: a body declared at exactly `MAX_REFUSAL_BYTES`
/// is read; one declared a byte larger is not.
#[tokio::test]
async fn a_429_body_declared_at_exactly_the_byte_bound_is_read() {
    for (length, quota) in [(MAX_REFUSAL_BYTES, true), (MAX_REFUSAL_BYTES + 1, false)] {
        let mut body = QUOTA_BODY.as_bytes().to_vec();
        body.resize(length, b' ');
        let (url, server) = serve(
            head(
                "429 Too Many Requests",
                &format!("Content-Length: {length}\r\n"),
            ),
            vec![body],
            Duration::ZERO,
        )
        .await;
        let (error, _) = refused(&quota_client(), &url).await;
        if quota {
            assert_quota(&error, &format!("declared {length}"));
        } else {
            assert_rate_limited(&error, &format!("declared {length}"));
        }
        assert_eq!(server.await.unwrap(), 1, "{length}");
    }
}

fn chunked(pieces: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = pieces
        .iter()
        .map(|piece| {
            let mut part = format!("{:x}\r\n", piece.len()).into_bytes();
            part.extend_from_slice(piece);
            part.extend_from_slice(b"\r\n");
            part
        })
        .collect();
    parts.push(b"0\r\n\r\n".to_vec());
    parts
}

/// Chunked transfer coding: a quota document in several chunks is read; the same code at the end
/// of a document one byte past the bound is not.
#[tokio::test]
async fn a_chunked_429_body_is_read_within_the_bound_and_not_past_it() {
    let (left, right) = QUOTA_BODY.as_bytes().split_at(10);
    let (url, server) = serve(
        head("429 Too Many Requests", "Transfer-Encoding: chunked\r\n"),
        chunked(&[left, right]),
        Duration::from_millis(10),
    )
    .await;
    let (error, _) = refused(&quota_client(), &url).await;
    assert_quota(&error, "chunked within the bound");
    assert_eq!(server.await.unwrap(), 1);

    let tail = br#"","error":{"code":"quota"}}"#;
    let mut long = br#"{"pad":""#.to_vec();
    long.resize(MAX_REFUSAL_BYTES + 1 - tail.len(), b'x');
    long.extend_from_slice(tail);
    assert_eq!(long.len(), MAX_REFUSAL_BYTES + 1);
    let (first, second) = long.split_at(8 * 1024);
    let (url, server) = serve(
        head("429 Too Many Requests", "Transfer-Encoding: chunked\r\n"),
        chunked(&[first, second]),
        Duration::from_millis(10),
    )
    .await;
    let (error, _) = refused(&quota_client(), &url).await;
    assert_rate_limited(&error, "chunked one byte past the bound, code at the end");
    assert_eq!(server.await.unwrap(), 1);
}

/// CRC-32 (IEEE), for a gzip member written by hand.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A valid gzip member holding `data` in one stored deflate block.
fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let length = u16::try_from(data.len()).unwrap();
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 0x01];
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&(!length).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out
}

/// The transport asks for no content coding and decodes none, so a gzip-coded quota document is
/// not JSON to it: the 429 keeps its class rather than being decoded past the byte bound.
#[tokio::test]
async fn a_gzip_coded_429_body_is_not_decoded_and_keeps_its_class() {
    let body = gzip_stored(QUOTA_BODY.as_bytes());
    let (url, server) = serve(
        head(
            "429 Too Many Requests",
            &format!(
                "Content-Encoding: gzip\r\nContent-Length: {}\r\n",
                body.len()
            ),
        ),
        vec![body],
        Duration::ZERO,
    )
    .await;
    let (error, _) = refused(&quota_client(), &url).await;
    assert_rate_limited(&error, "gzip");
    assert_eq!(server.await.unwrap(), 1);
}

/// On the wire, as the SDKs read it: two field lines combine (`false, true`) and are no signal;
/// a value with other letter case is no signal; on a quota 429, `true` does not undo the rule.
#[tokio::test]
async fn the_signal_on_the_wire_is_one_exact_false_and_only_narrows() {
    let body = r#"{"error":{"code":"upstream-failed"}}"#;
    for (fields, retriable) in [
        ("x-should-retry: false\r\nx-should-retry: true\r\n", true),
        ("x-should-retry: False\r\n", true),
        ("x-should-retry:    false   \r\n", false),
    ] {
        let (url, server) = serve(
            head(
                "502 Bad Gateway",
                &format!("{fields}Content-Length: {}\r\n", body.len()),
            ),
            vec![body.as_bytes().to_vec()],
            Duration::ZERO,
        )
        .await;
        let (error, _) = refused(&HttpClient::new(Limits::default()).unwrap(), &url).await;
        assert_eq!(
            (error.code, error.dispatch, error.retriable),
            (ErrorCode::Transport, Dispatch::Unknown, retriable),
            "`{fields}`"
        );
        assert_eq!(server.await.unwrap(), 1, "`{fields}`");
    }
    for fields in ["x-should-retry: true\r\n", "x-should-retry: false\r\n"] {
        let (url, server) = serve(
            head(
                "429 Too Many Requests",
                &format!("{fields}Content-Length: {}\r\n", QUOTA_BODY.len()),
            ),
            vec![QUOTA_BODY.as_bytes().to_vec()],
            Duration::ZERO,
        )
        .await;
        let (error, _) = refused(&quota_client(), &url).await;
        assert_quota(&error, fields);
        assert_eq!(server.await.unwrap(), 1, "`{fields}`");
    }
}
