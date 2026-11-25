#![cfg(all(feature = "codex-renewal", unix))]
//! Adversary pass 1, wave 2026-11-09, unit 5 (`story:codex-renewal-blocking-io-bound`): the
//! write-back re-read now goes through `read_current`, with its own size limit. Fixture files under
//! `CARGO_TARGET_TMPDIR` and a one-shot token endpoint on 127.0.0.1 only.
use llm_core::Cancel;
use llm_credentials::{
    MAX_SECRET_BYTES, SecretRef,
    codex::{CodexAuthFile, CodexRenewal, Renewal},
};
use serde_json::{Value, json};
use std::{
    fs,
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const NOW: u64 = 1_790_985_600;
const OLD_REFRESH: &str = "fixture-refresh-token-one";
const NEW_REFRESH: &str = "fixture-refresh-token-two";
const STALL: Duration = Duration::from_mins(2);

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let bits = (u32::from(group[0]) << 16) | (u32::from(group[1]) << 8) | u32::from(group[2]);
        for index in 0..=chunk.len() {
            out.push(char::from(
                ALPHABET[(bits >> (18 - 6 * index)) as usize & 63],
            ));
        }
    }
    out
}

fn token(offset: i64, subject: &str) -> String {
    let exp = i64::try_from(NOW).unwrap() + offset;
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(format!(r#"{{"exp":{exp},"sub":"{subject}"}}"#).as_bytes()),
        base64url(b"fixture-signature"),
    )
}

/// Answers the first request with `answer` and returns the URL to send it to.
async fn endpoint(answer: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => bytes.extend_from_slice(&buffer[..read]),
            }
        }
        let answer = answer.to_string();
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            answer.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(answer.as_bytes()).await;
    });
    url
}

/// Boundary of the re-read's size limit: a login of exactly `MAX_SECRET_BYTES`, which the first
/// read accepts, is renewed. The re-read applies its own limit (`read_current`), and nothing in
/// the suite drives a due login at the limit through the write-back, so `<=` turned `<` there
/// refuses this login `ChangedDuringRenewal` after the endpoint has rotated its refresh token, and
/// every test stays green.
#[tokio::test]
async fn a_login_of_exactly_the_size_limit_is_renewed_and_written_back() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let path = dir.path().join("auth.json");
    let fresh = token(3600, "fresh");
    let head = format!(
        "{{\"tokens\": {{\"access_token\": \"{}\", \"refresh_token\": \"{OLD_REFRESH}\"}}, \"last_refresh\": \"2026-10-01T00:00:00Z\", \"padding\": \"",
        token(-60, "expired-subject-longer-than-fresh"),
    );
    let tail = "\"}\n";
    let padding = "p".repeat(MAX_SECRET_BYTES - head.len() - tail.len());
    let before = format!("{head}{padding}{tail}");
    assert_eq!(before.len(), MAX_SECRET_BYTES);
    fs::write(&path, &before).unwrap();

    let url = endpoint(json!({"access_token": fresh, "refresh_token": NEW_REFRESH})).await;
    let outcome = tokio::time::timeout(
        STALL,
        CodexAuthFile::new(SecretRef::new("codex-login").unwrap(), &path)
            .with_clock(|| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW))
            .renew(
                &CodexRenewal::new()
                    .unwrap()
                    .with_endpoint(&url, "fixture-client")
                    .with_margin(Duration::from_mins(15)),
                &Cancel::new(),
            ),
    )
    .await
    .expect("the renewal stalled")
    .unwrap_or_else(|error| panic!("a login at the size limit was refused: {error}"));
    assert!(matches!(outcome, Renewal::Renewed(_)), "{outcome:?}");

    let after = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(parsed["tokens"]["access_token"], json!(fresh));
    assert_eq!(parsed["tokens"]["refresh_token"], json!(NEW_REFRESH));
    assert_eq!(parsed["padding"], json!(padding));
}
