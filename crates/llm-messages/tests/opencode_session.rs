//! story:opencode-go-session.
//!
//! `OpenCode` Go refuses a Messages request without `x-opencode-session` (HTTP 400,
//! `MissingSessionID`), and its page asks a client to "Send a stable session ID in
//! `x-opencode-session` for each conversation" (<https://opencode.ai/docs/go/>). A client whose
//! binding's base URL is `OpenCode` Go mints one identifier at construction and sends it on every
//! request; a caller may name the session instead. Every other endpoint sends nothing new.
//!
//! Local sockets, fixture credentials and fixture documents only; no provider is contacted.
mod support;

use llm_core::{Cancel, Id, Item, Model, TurnRequest, VecSink};
use llm_http::{HttpClient, Limits};
use llm_messages::{MessagesClient, OPENCODE_SESSION_HEADER, is_opencode_go};
use llm_providers::{BaseUrl, Binding};
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// The base URL `OpenCode` Go's page names for its Messages endpoint, without the `messages` path.
const OPENCODE_GO: &str = "https://opencode.ai/inference/go/anthropic/v1";

const STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model\",\"content\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n\
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

fn base(url: &str) -> BaseUrl {
    BaseUrl::new(url).unwrap_or_else(|error| panic!("`{url}` is a base URL: {error}"))
}

fn bound(url: &str) -> Binding {
    binding_with(url, capabilities())
}

fn client(binding: Binding) -> MessagesClient {
    let total = Duration::from_secs(10);
    MessagesClient::new(
        binding,
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .unwrap(),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding")
}

#[test]
fn the_header_name_is_the_one_opencode_go_documents() {
    assert_eq!(OPENCODE_SESSION_HEADER, "x-opencode-session");
}

#[test]
fn an_https_opencode_ai_base_under_inference_go_is_opencode_go() {
    for url in [
        OPENCODE_GO,
        "https://opencode.ai/inference/go/anthropic/v1/",
        "https://opencode.ai/inference/go/",
        // Normalised to `/inference/go/` by the base URL.
        "https://opencode.ai/inference/go",
        // The `url` crate lowercases the host and drops the scheme's default port.
        "https://OpenCode.AI/inference/go/anthropic/v1",
        "https://opencode.ai:443/inference/go/anthropic/v1",
    ] {
        assert!(is_opencode_go(&base(url)), "{url} is OpenCode Go");
    }
}

#[test]
fn every_look_alike_and_every_other_endpoint_is_not_opencode_go() {
    for url in [
        "http://opencode.ai/inference/go/anthropic/v1",
        "https://opencode.ai.example/inference/go/anthropic/v1",
        "https://www.opencode.ai/inference/go/anthropic/v1",
        "https://opencode.ai:8443/inference/go/anthropic/v1",
        "https://opencode.ai/zen/v1",
        "https://opencode.ai/inference/gox/anthropic/v1",
        "https://opencode.ai/inference/",
        "https://opencode.ai/",
        // Dot segments are resolved before the check: this path is `/inference/zen/v1/`.
        "https://opencode.ai/inference/go/../zen/v1",
        "https://api.anthropic.com/v1/",
        "https://messages.example.invalid/v1",
        "https://proxy.example.invalid/https://opencode.ai/inference/go/",
    ] {
        assert!(!is_opencode_go(&base(url)), "{url} is not OpenCode Go");
    }
}

#[test]
fn one_client_keeps_one_minted_session_and_two_clients_mint_two() {
    let first = client(bound(OPENCODE_GO));
    let second = client(bound(OPENCODE_GO));
    let (name, value) = first
        .session_header()
        .expect("an OpenCode Go client sends a session");
    assert_eq!(name, OPENCODE_SESSION_HEADER);
    assert_eq!(
        first.session_header(),
        Some((name, value)),
        "two reads of one client agree"
    );
    let (_, other) = second
        .session_header()
        .expect("an OpenCode Go client sends a session");
    assert_ne!(value, other, "two clients mint two sessions");
    for minted in [value, other] {
        assert!(
            Id::new(minted).is_ok(),
            "`{minted}` is a valid identifier: printable ASCII, at most 256 bytes"
        );
    }
}

#[test]
fn many_clients_in_one_process_never_share_a_session() {
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let client = client(bound(OPENCODE_GO));
        let (_, value) = client.session_header().expect("a minted session");
        assert!(seen.insert(value.to_owned()), "`{value}` was minted twice");
    }
}

#[test]
fn a_named_session_is_sent_as_given() {
    let client = client(bound(OPENCODE_GO)).with_session(Id::new("conversation-42").unwrap());
    assert_eq!(
        client.session_header(),
        Some((OPENCODE_SESSION_HEADER, "conversation-42"))
    );
}

#[test]
fn another_endpoint_has_no_session_even_when_one_is_named() {
    for url in [
        "https://api.anthropic.com/v1/",
        "http://opencode.ai/inference/go/anthropic/v1",
    ] {
        assert_eq!(client(bound(url)).session_header(), None, "{url}");
        assert_eq!(
            client(bound(url))
                .with_session(Id::new("conversation-42").unwrap())
                .session_header(),
            None,
            "{url}: a name sends nothing where no session is sent"
        );
    }
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
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

#[tokio::test]
async fn a_turn_against_another_endpoint_sends_no_opencode_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("the client never opened a connection")
            .unwrap();
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
    let client = client(bound(&url)).with_session(Id::new("conversation-42").unwrap());
    let request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    client
        .turn(&request, &mut VecSink::new(16, 4096), &Cancel::new())
        .await
        .expect("a decoded turn");
    let captured = server.await.unwrap();
    let (head, _) = captured.split_once("\r\n\r\n").expect("a head and a body");
    let head = head.to_ascii_lowercase();
    assert!(head.starts_with("post /v1/messages "), "{head}");
    assert!(!head.contains(OPENCODE_SESSION_HEADER), "{head}");
    assert!(!head.contains("conversation-42"), "{head}");
}
