//! Two Messages turns recorded from `api.anthropic.com` on 2026-10-21 under a subscription
//! token, replayed from `127.0.0.1`. No provider is contacted.
//!
//! The same request was sent twice in one process: the first turn wrote the prompt prefix to the
//! cache and the second read it back. `fixtures/live/` holds each turn's recorded head and event
//! stream, scrubbed as `fixtures/live/README.md` lists. The replay serves each stream in fixed
//! 256-byte chunks.
mod support;

use llm_core::{Cancel, Item, Model, StopReason, TurnRequest, VecSink};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use llm_providers::{Binding, BindingDocument};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const WRITE_HEAD: &str = include_str!("fixtures/live/messages-cache-write-head.txt");
const WRITE_EVENTS: &[u8] = include_bytes!("fixtures/live/messages-cache-write.sse");
const READ_HEAD: &str = include_str!("fixtures/live/messages-cache-read-head.txt");
const READ_EVENTS: &[u8] = include_bytes!("fixtures/live/messages-cache-read.sse");

const UPSTREAM_MODEL: &str = "claude-haiku-4-5-20251001";
/// The prefix both turns cached: the reported input less the 3 uncached tokens.
const CACHED: u64 = 13_027;

/// The fixture binding under a `subscription-oauth` account, as the recording ran.
fn subscription(base_url: &str) -> Binding {
    let mut document =
        serde_json::to_value(binding_with(base_url, capabilities()).declaration()).unwrap();
    let account = document["account"].as_object_mut().unwrap();
    account.insert("auth_kind".to_owned(), json!("subscription-oauth"));
    account.insert("billing_kind".to_owned(), json!("subscription"));
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

/// The recorded user message: 400 numbered lines, long enough for a cache breakpoint to apply.
fn prompt() -> String {
    let mut lines = vec![
        "Below is a numbered reference list. Read it, then reply with the single word: ok"
            .to_owned(),
        String::new(),
    ];
    for n in 1..=400 {
        lines.push(format!(
            "{n:04}. Entry {n} records that a portable model client sends one request, reads one \
             event stream and reports the usage counters the provider returned."
        ));
    }
    lines.push(String::new());
    lines.push("Reply with the single word: ok".to_owned());
    lines.join("\n")
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user(prompt())]);
    request.max_output_tokens = Some(64);
    request
}

/// The recorded head with the framing headers the scrub dropped put back, CRLF-terminated.
fn head(recorded: &str) -> Vec<u8> {
    let mut head = String::new();
    for line in recorded.lines() {
        head.push_str(line);
        head.push_str("\r\n");
    }
    head.push_str("transfer-encoding: chunked\r\nconnection: close\r\n\r\n");
    head.into_bytes()
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = header
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

/// Serves the two recorded turns, one connection each, and returns what each request carried.
async fn replay() -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut captured = Vec::new();
        for (recorded, events) in [(WRITE_HEAD, WRITE_EVENTS), (READ_HEAD, READ_EVENTS)] {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
                .await
                .expect("the client never opened a connection")
                .unwrap();
            captured.push(read_request(&mut socket).await);
            socket.write_all(&head(recorded)).await.unwrap();
            for chunk in events.chunks(256) {
                let mut frame = format!("{:x}\r\n", chunk.len()).into_bytes();
                frame.extend_from_slice(chunk);
                frame.extend_from_slice(b"\r\n");
                socket.write_all(&frame).await.unwrap();
            }
            socket.write_all(b"0\r\n\r\n").await.unwrap();
            socket.shutdown().await.unwrap();
        }
        captured
    });
    (url, server)
}

fn body(captured: &str) -> Value {
    serde_json::from_str(
        captured
            .split_once("\r\n\r\n")
            .expect("a head and a body")
            .1,
    )
    .expect("a JSON body")
}

/// The first recorded turn reports the prefix written to the cache and the second the same
/// prefix read back, each as the live run decoded it.
#[tokio::test]
async fn the_recorded_turns_replay_a_cache_write_and_then_a_cache_read() {
    let (url, server) = replay().await;
    let total = Duration::from_secs(10);
    let client = MessagesClient::new(
        subscription(&url),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .unwrap(),
        Arc::new(StaticResolver::new("fixture-subscription-token")),
    )
    .expect("a Messages binding");

    let mut outcomes = Vec::new();
    for _ in 0..2 {
        let mut sink = VecSink::new(64, 4096);
        let outcome = client
            .turn(&request(), &mut sink, &Cancel::new())
            .await
            .expect("the recorded stream completes its turn");
        assert_eq!(sink.text(), "ok");
        outcomes.push(outcome);
    }
    let captured = server.await.unwrap();

    // Both turns sent the same body, with the rolling breakpoint on the prompt.
    assert_eq!(body(&captured[0]), body(&captured[1]));
    assert_eq!(
        body(&captured[0])["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );

    for (outcome, (created, read)) in outcomes.iter().zip([(CACHED, 0), (0, CACHED)]) {
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(
            outcome.items,
            vec![Item::AssistantText {
                text: "ok".to_owned()
            }]
        );
        let observation = &outcome.observation;
        assert!(observation.final_usage);
        assert_eq!(
            observation.upstream_model.as_ref().map(ToString::to_string),
            Some(UPSTREAM_MODEL.to_owned())
        );
        let usage = observation.usage.as_ref().expect("reported usage");
        assert_eq!(usage.input_tokens, Some(3 + CACHED));
        assert_eq!(usage.output_tokens, Some(4));
        assert_eq!(usage.cache_creation_input_tokens, Some(created));
        assert_eq!(usage.cached_input_tokens, Some(read));
    }
}
