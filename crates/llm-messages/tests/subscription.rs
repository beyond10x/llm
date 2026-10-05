//! story:anthropic-access, Harness parity C21, M5, M6 and M44 (Harness
//! `harness-messages/src/lib.rs:83`-`93`, `:202`-`205`; `src/project.rs:47`, `:528`-`530`;
//! `tests/contract.rs:143`-`227`, at Harness `origin/main` `3169042f`).
//!
//! Under a `subscription-oauth` account the Messages client sends the caller's subscription token
//! as `authorization: Bearer <token>` plus `anthropic-beta: oauth-2025-04-20`, and `system` opens
//! with the subscription client preamble as its own unmarked block. The token is resolved through
//! the account's secret reference on every turn, so a rotated token is used by the next turn;
//! llm reads no login file. API-key and plain bearer accounts send neither the beta header nor
//! the preamble. The pinned request is `contracts/anthropic/subscription-request.json`.
//!
//! Local sockets, fixture credentials and fixture documents only; no provider is contacted.
mod support;

use llm_core::{
    BoxFuture, CallId, Cancel, Item, Model, Sampling, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_messages::{MessagesClient, encode_request};
use llm_providers::{Binding, BindingDocument};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{binding, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// The exact text Harness sends (`harness-messages/src/project.rs:47`), written out rather than
/// imported so the pin cannot move with the constant.
const PREAMBLE: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const REFERENCE: &str = "operator-subscription";

const STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":11,\"output_tokens\":8}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

/// The fixture binding's declaration with its account replaced, parsed back from JSON so the
/// spelling under test is the declaration vocabulary's.
fn rebound(base_url: &str, auth_kind: &str, billing_kind: &str) -> Binding {
    let mut document =
        serde_json::to_value(binding_with(base_url, capabilities()).declaration()).unwrap();
    let account = document["account"].as_object_mut().unwrap();
    account.insert("auth_kind".to_owned(), json!(auth_kind));
    account.insert("billing_kind".to_owned(), json!(billing_kind));
    account.insert("secret_reference_id".to_owned(), json!(REFERENCE));
    account.remove("api_key_header");
    serde_json::from_value::<BindingDocument>(document)
        .unwrap_or_else(|error| panic!("`{auth_kind}` is an account auth kind: {error}"))
        .bind()
        .unwrap_or_else(|error| panic!("a `{auth_kind}` account binds on Messages: {error}"))
}

fn subscription(base_url: &str) -> Binding {
    rebound(base_url, "subscription-oauth", "subscription")
}

fn subscription_binding() -> Binding {
    subscription("https://messages.example.invalid/v1")
}

/// Answers a new token on every resolution and records the reference each one was asked for.
#[derive(Default)]
struct Rotating {
    asked: Mutex<Vec<String>>,
    answers: AtomicUsize,
}

impl SecretResolver for Rotating {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.asked
            .lock()
            .unwrap()
            .push(reference.as_str().to_owned());
        let n = self.answers.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(format!("subscription-token-{n}").into_bytes())?,
                version: SecretVersion::new(format!("generation-{n}"))?,
            })
        })
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

/// Serves `turns` requests one connection each and returns what each one carried.
async fn serve(turns: usize) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut captured = Vec::new();
        for _ in 0..turns {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
                .await
                .expect("the client never opened a connection")
                .unwrap();
            captured.push(read_request(&mut socket).await);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            socket.write_all(STREAM).await.unwrap();
            socket.shutdown().await.unwrap();
        }
        captured
    });
    (url, server)
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
    .expect("a Messages binding")
}

fn short_request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    "Stay terse".clone_into(&mut request.instructions);
    request.max_output_tokens = Some(512);
    request
}

/// The head lowercased, and the body parsed.
fn split(captured: &str) -> (String, Value) {
    let (head, body) = captured.split_once("\r\n\r\n").expect("a head and a body");
    (
        head.to_ascii_lowercase(),
        serde_json::from_str(body).expect("a JSON body"),
    )
}

fn wire(request: &TurnRequest, binding: &Binding) -> Value {
    serde_json::from_slice(&encode_request(request, binding).expect("projected")).expect("JSON")
}

fn markers(value: &Value) -> usize {
    value.to_string().matches("cache_control").count()
}

#[tokio::test]
async fn a_subscription_turn_presents_the_token_as_a_bearer_with_the_oauth_beta_and_the_preamble() {
    let (url, server) = serve(1).await;
    let resolver = Arc::new(Rotating::default());
    let client = client(subscription(&url), resolver.clone());
    client
        .turn(
            &short_request(),
            &mut VecSink::new(16, 4096),
            &Cancel::new(),
        )
        .await
        .expect("a decoded turn");
    let captured = server.await.unwrap();
    let (head, body) = split(&captured[0]);

    assert!(head.starts_with("post /v1/messages "), "{head}");
    assert!(
        head.contains("\r\nauthorization: bearer subscription-token-1\r\n"),
        "{head}"
    );
    assert!(
        head.contains(&format!("\r\nanthropic-beta: {OAUTH_BETA}\r\n")),
        "{head}"
    );
    assert!(
        head.contains("\r\nanthropic-version: 2023-06-01\r\n"),
        "{head}"
    );
    assert!(!head.contains("x-api-key:"), "{head}");
    assert_eq!(head.matches("anthropic-beta:").count(), 1, "{head}");
    assert_eq!(*resolver.asked.lock().unwrap(), [REFERENCE.to_owned()]);
    assert_eq!(
        body["system"],
        json!([
            {"type": "text", "text": PREAMBLE},
            {"type": "text", "text": "Stay terse", "cache_control": {"type": "ephemeral"}}
        ]),
        "{body}"
    );
}

/// C21 and the acceptance's rotation: the token is resolved through the declared reference on
/// every turn, never cached, so the turn after a rotation presents the rotated token.
#[tokio::test]
async fn a_rotated_subscription_token_is_presented_on_the_next_turn() {
    let (url, server) = serve(2).await;
    let resolver = Arc::new(Rotating::default());
    let client = client(subscription(&url), resolver.clone());
    for _ in 0..2 {
        client
            .turn(
                &short_request(),
                &mut VecSink::new(16, 4096),
                &Cancel::new(),
            )
            .await
            .expect("a decoded turn");
    }
    let captured = server.await.unwrap();
    for (n, request) in captured.iter().enumerate() {
        let (head, _) = split(request);
        assert!(
            head.contains(&format!(
                "\r\nauthorization: bearer subscription-token-{}\r\n",
                n + 1
            )),
            "{head}"
        );
        assert!(
            head.contains(&format!("\r\nanthropic-beta: {OAUTH_BETA}\r\n")),
            "{head}"
        );
    }
    assert_eq!(
        *resolver.asked.lock().unwrap(),
        [REFERENCE.to_owned(), REFERENCE.to_owned()]
    );
}

/// A plain bearer on Messages sends the same authorization header, and nothing else of the
/// subscription presentation: the two kinds stay distinct (C8).
#[tokio::test]
async fn a_bearer_turn_carries_neither_the_oauth_beta_nor_the_preamble() {
    let (url, server) = serve(1).await;
    let resolver = Arc::new(Rotating::default());
    let client = client(rebound(&url, "bearer", "metered"), resolver);
    client
        .turn(
            &short_request(),
            &mut VecSink::new(16, 4096),
            &Cancel::new(),
        )
        .await
        .expect("a decoded turn");
    let captured = server.await.unwrap();
    let (head, body) = split(&captured[0]);
    assert!(
        head.contains("\r\nauthorization: bearer subscription-token-1\r\n"),
        "{head}"
    );
    assert!(!head.contains("anthropic-beta:"), "{head}");
    assert_eq!(
        body["system"],
        json!([{"type": "text", "text": "Stay terse", "cache_control": {"type": "ephemeral"}}]),
        "{body}"
    );
}

/// Block 0 is the preamble, exactly and alone; the instruction follows and keeps the breakpoint.
/// Every other shape Harness measured on 2026-08-30 was answered 429 with no rate-limit headers.
#[test]
fn the_preamble_is_its_own_unmarked_block_before_the_marked_instruction() {
    let body = wire(&short_request(), &subscription_binding());
    let system = body["system"].as_array().expect("a block list");
    assert_eq!(system.len(), 2, "{body}");
    assert_eq!(system[0]["type"], json!("text"));
    assert_eq!(system[0]["text"], json!(PREAMBLE));
    assert_eq!(
        system[0].as_object().unwrap().len(),
        2,
        "the preamble block carries type and text and nothing else: {body}"
    );
    assert_eq!(system[1]["text"], json!("Stay terse"));
    assert_eq!(system[1]["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(markers(&body), 2, "{body}");
}

/// llm sends no `system` for an empty instruction; a subscription token still needs the
/// preamble, alone and unmarked, and the rolling breakpoint stays on the tail.
#[test]
fn an_empty_instruction_under_a_subscription_opens_with_the_preamble_alone() {
    let mut request = short_request();
    request.instructions.clear();
    let body = wire(&request, &subscription_binding());
    assert_eq!(
        body["system"],
        json!([{"type": "text", "text": PREAMBLE}]),
        "{body}"
    );
    assert_eq!(
        body["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(markers(&body), 1, "{body}");
}

/// The fixture binding is an API-key account: no preamble, before or after this story.
#[test]
fn an_api_key_request_carries_no_preamble() {
    let body = wire(&short_request(), &binding());
    assert_eq!(
        body["system"],
        json!([{"type": "text", "text": "Stay terse", "cache_control": {"type": "ephemeral"}}]),
        "{body}"
    );
    assert!(!body.to_string().contains(PREAMBLE), "{body}");
}

fn canonical(binding: &Binding) -> TurnRequest {
    let mut request = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("Summarise the log"),
            Item::Opaque {
                provenance: binding.provenance().clone(),
                payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
            },
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"query":"errors"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call-1").unwrap(),
                output: json!("not found"),
                failed: true,
            },
        ],
    );
    "Stay terse".clone_into(&mut request.instructions);
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: "Look up a record".to_owned(),
        input_schema: json!({"type":"object"}),
    }];
    request.tool_choice = ToolChoice::Named(ToolName::new("lookup").unwrap());
    request.max_output_tokens = Some(1024);
    request.sampling = Sampling {
        temperature: Some(0.5),
        top_p: Some(0.8),
        reasoning_effort: Some("high".to_owned()),
    };
    request
}

/// The pinned bytes, read at run time from this checkout, without the file's one final newline.
fn pinned() -> Vec<u8> {
    let path = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../contracts/anthropic/subscription-request.json");
    let mut bytes = std::fs::read(path).expect("the pinned subscription request");
    assert_eq!(bytes.pop(), Some(b'\n'), "the fixture ends in one newline");
    bytes
}

/// M44: every field the projection sends, under a subscription token, byte for byte.
#[test]
fn the_subscription_request_matches_its_pinned_fixture() {
    let binding = subscription_binding();
    let actual = encode_request(&canonical(&binding), &binding).expect("projected");
    assert_eq!(
        String::from_utf8(actual).unwrap(),
        String::from_utf8(pinned()).unwrap(),
        "the exact subscription request bytes changed; cut a contract and say so"
    );
}

/// The fixture is canonical and differs from the API-key request for the same turn by exactly
/// its first `system` block, so the pin cannot drift from the projection it extends.
#[test]
fn the_pinned_fixture_is_the_api_key_request_plus_the_preamble_block() {
    let pinned = pinned();
    let mut fixture: Value = serde_json::from_slice(&pinned).expect("JSON");
    assert_eq!(
        serde_json::to_vec(&fixture).unwrap(),
        pinned,
        "compact, sorted keys"
    );
    let removed = fixture["system"].as_array_mut().unwrap().remove(0);
    assert_eq!(removed, json!({"type": "text", "text": PREAMBLE}));
    let api_key = binding();
    assert_eq!(
        serde_json::to_vec(&fixture).unwrap(),
        encode_request(&canonical(&api_key), &api_key).expect("projected")
    );
}
