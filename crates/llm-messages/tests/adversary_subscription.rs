//! Adversary pass, story:anthropic-access (wave 2026-10-05-w48).
//!
//! Attacks the Messages half of the subscription presentation where the unit's own tests do not
//! reach: a redirect to another host, a refused turn followed by the caller's retry with a rotated
//! token, the preamble against every instruction shape (empty, the preamble itself, near misses),
//! and the beta header keyed on billing instead of on the auth kind.
//!
//! Loopback sockets and fixture tokens only; no provider is contacted and no file is read.
mod support;

use llm_core::{Cancel, ErrorCode, Item, Model, TurnRequest, VecSink};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_messages::{MessagesClient, encode_request};
use llm_providers::{Binding, BindingDocument};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// Written out, not imported, so a moved constant cannot move the pin.
const PREAMBLE: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const TOKEN_PREFIX: &str = "adversary-subscription-token-";

const STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_adv\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model\",\"content\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

/// Answers `TOKEN_PREFIX<n>` on its n-th resolution.
#[derive(Default)]
struct Rotating {
    answers: AtomicUsize,
}

impl SecretResolver for Rotating {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        let n = self.answers.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(format!("{TOKEN_PREFIX}{n}").into_bytes())?,
                version: SecretVersion::new(format!("generation-{n}"))?,
            })
        })
    }
}

fn rebound(base_url: &str, auth_kind: &str, billing_kind: &str) -> Binding {
    let mut document =
        serde_json::to_value(binding_with(base_url, capabilities()).declaration()).unwrap();
    let account = document["account"].as_object_mut().unwrap();
    account.insert("auth_kind".to_owned(), json!(auth_kind));
    account.insert("billing_kind".to_owned(), json!(billing_kind));
    account.insert(
        "secret_reference_id".to_owned(),
        json!("operator-subscription"),
    );
    account.remove("api_key_header");
    serde_json::from_value::<BindingDocument>(document)
        .unwrap()
        .bind()
        .unwrap()
}

fn client(binding: Binding, resolver: Arc<Rotating>) -> MessagesClient {
    let total = Duration::from_secs(10);
    MessagesClient::new(
        binding,
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .unwrap(),
        resolver,
    )
    .unwrap()
}

fn request(instructions: &str) -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    instructions.clone_into(&mut request.instructions);
    request.max_output_tokens = Some(256);
    request
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |v| v.trim().parse().unwrap());
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }
}

/// A 307 to another loopback host never carries the subscription token there, and the refusal
/// names no token.
#[tokio::test]
async fn a_redirect_to_another_host_never_receives_the_subscription_token() {
    let first = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let elsewhere = format!("http://{}/v1/messages", second.local_addr().unwrap());
    let url = format!("http://{}/v1", first.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = first.accept().await.unwrap();
        let captured = read_request(&mut socket).await;
        let answer = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {elsewhere}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        socket.write_all(answer.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
        captured
    });
    let resolver = Arc::new(Rotating::default());
    let client = client(
        rebound(&url, "subscription-oauth", "subscription"),
        resolver,
    );
    let error = client
        .turn(
            &request("Stay terse"),
            &mut VecSink::new(16, 4096),
            &Cancel::new(),
        )
        .await
        .expect_err("a redirect is not a turn");
    let captured = server.await.unwrap();
    assert!(captured.contains(&format!("{TOKEN_PREFIX}1")), "{captured}");
    let followed = tokio::time::timeout(Duration::from_millis(300), second.accept()).await;
    assert!(followed.is_err(), "the redirect target was contacted");
    assert_eq!(error.code, ErrorCode::Refused, "{error}");
    assert!(
        !format!("{error} {error:?}").contains(TOKEN_PREFIX),
        "{error:?}"
    );
}

/// A turn refused 429 and the caller's next turn: the next one resolves again, presents the
/// rotated token, and both carry the beta header. The refusal names no token.
#[tokio::test]
async fn a_retry_after_a_refused_subscription_turn_presents_the_rotated_token_with_the_beta() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut captured = Vec::new();
        for answer in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            captured.push(read_request(&mut socket).await);
            if answer == 0 {
                socket
                    .write_all(
                        b"HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .await
                    .unwrap();
            } else {
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
                socket.write_all(STREAM).await.unwrap();
            }
            socket.shutdown().await.unwrap();
        }
        captured
    });
    let resolver = Arc::new(Rotating::default());
    let client = client(
        rebound(&url, "subscription-oauth", "subscription"),
        resolver.clone(),
    );
    let refused = client
        .turn(
            &request("Stay terse"),
            &mut VecSink::new(16, 4096),
            &Cancel::new(),
        )
        .await
        .expect_err("429");
    assert_eq!(refused.code, ErrorCode::RateLimited, "{refused}");
    assert!(!format!("{refused} {refused:?}").contains(TOKEN_PREFIX));
    client
        .turn(
            &request("Stay terse"),
            &mut VecSink::new(16, 4096),
            &Cancel::new(),
        )
        .await
        .expect("the retried turn");
    let captured = server.await.unwrap();
    for (n, captured) in captured.iter().enumerate() {
        let head = captured
            .split("\r\n\r\n")
            .next()
            .unwrap()
            .to_ascii_lowercase();
        assert!(
            head.contains(&format!(
                "\r\nauthorization: bearer {TOKEN_PREFIX}{}\r\n",
                n + 1
            )),
            "{head}"
        );
        assert_eq!(
            head.matches("\r\nanthropic-beta: oauth-2025-04-20\r\n")
                .count(),
            1,
            "{head}"
        );
        assert!(!head.contains("x-api-key"), "{head}");
    }
    assert_eq!(resolver.answers.load(Ordering::SeqCst), 2);
}

fn system(binding: &Binding, instructions: &str) -> Option<Value> {
    let body: Value =
        serde_json::from_slice(&encode_request(&request(instructions), binding).unwrap()).unwrap();
    body.get("system").cloned()
}

/// For every instruction shape, a subscription request opens with the preamble block exactly
/// and alone, unmarked; the caller's instruction, whatever it says, is the one marked block after
/// it. No other presentation ever gains a block the caller did not write.
#[test]
fn the_preamble_opens_system_for_every_instruction_shape_and_only_under_a_subscription() {
    let shapes = [
        String::new(),
        "Stay terse".to_owned(),
        PREAMBLE.to_owned(),
        format!("{PREAMBLE}\n"),
        format!(" {PREAMBLE}"),
        format!("{PREAMBLE} Stay terse"),
        "x".repeat(20_000),
    ];
    let subscription = rebound(
        "https://messages.example.invalid/v1",
        "subscription-oauth",
        "subscription",
    );
    let bearer = rebound(
        "https://messages.example.invalid/v1",
        "bearer",
        "subscription",
    );
    let api_key = support::binding();
    for instructions in &shapes {
        let blocks = system(&subscription, instructions).expect("a subscription sends system");
        let blocks = blocks.as_array().unwrap();
        assert_eq!(
            blocks[0],
            json!({"type": "text", "text": PREAMBLE}),
            "{instructions:?}"
        );
        if instructions.is_empty() {
            assert_eq!(blocks.len(), 1);
        } else {
            assert_eq!(
                blocks[1..],
                [json!({"type":"text","text":instructions,"cache_control":{"type":"ephemeral"}})],
                "{instructions:?}"
            );
        }
        for other in [&bearer, &api_key] {
            let blocks = system(other, instructions);
            if instructions.is_empty() {
                assert_eq!(blocks, None);
            } else {
                assert_eq!(
                    blocks,
                    Some(
                        json!([{"type":"text","text":instructions,"cache_control":{"type":"ephemeral"}}])
                    ),
                    "{instructions:?}"
                );
            }
        }
    }
}

/// The beta header follows the auth kind, not the billing kind: a plain bearer billed as a
/// subscription (the Codex shape) sends no OAuth beta and no preamble.
#[tokio::test]
async fn a_bearer_billed_as_a_subscription_sends_neither_the_beta_nor_the_preamble() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let captured = read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(STREAM).await.unwrap();
        socket.shutdown().await.unwrap();
        captured
    });
    let client = client(
        rebound(&url, "bearer", "subscription"),
        Arc::new(Rotating::default()),
    );
    client
        .turn(
            &request("Stay terse"),
            &mut VecSink::new(16, 4096),
            &Cancel::new(),
        )
        .await
        .expect("a turn");
    let captured = server.await.unwrap();
    let (head, body) = captured.split_once("\r\n\r\n").unwrap();
    let head = head.to_ascii_lowercase();
    assert!(
        head.contains(&format!("\r\nauthorization: bearer {TOKEN_PREFIX}1\r\n")),
        "{head}"
    );
    assert!(!head.contains("anthropic-beta"), "{head}");
    assert!(!body.contains(PREAMBLE), "{body}");
}
