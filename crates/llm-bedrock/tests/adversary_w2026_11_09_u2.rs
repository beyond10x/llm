//! Adversary, wave 2026-11-09, llm unit 2 (story:bedrock-retry-invariant-held).
//!
//! The unit's cases call a `toolUse` / `tool_use` block start "silent output", but that start
//! shows the caller a `ToolCallStarted`. These cases drive output the caller is shown nothing
//! of (a redacted or signature-only reasoning delta on Converse, a `redacted_thinking` or empty
//! `thinking` block on InvokeModel), a stop with no content, and every transient exception type,
//! on both wires. Every frame is synthetic.

use base64::{Engine, engine::general_purpose::STANDARD};
use llm_bedrock::{
    Api, AwsCredentials, BedrockClient, BedrockTarget, CredentialSource,
    eventstream::{HeaderValue, Message, encode_message},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol,
    Provenance, StreamEvent, TurnRequest, VecSink,
};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use url::Url;

const EVENT_STREAM: &str = "application/vnd.amazon.eventstream";
const TRANSIENT: [&str; 5] = [
    "throttlingException",
    "serviceUnavailableException",
    "internalServerException",
    "modelNotReadyException",
    "modelTimeoutException",
];

struct Keys;

impl CredentialSource for Keys {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async { Ok(AwsCredentials::new("AKIDFIXTURE", "fixture-secret", None, None)) })
    }
}

/// Serves one connection: reads the request head and body, writes `head` then `body`, closes.
async fn serve(head: String, body: Vec<u8>) -> (Url, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let end = loop {
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break end;
            }
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&buffer[..read]);
        };
        let head_text = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
        let length: usize = head_text
            .split("\r\n")
            .find_map(|line| line.strip_prefix("content-length:"))
            .map_or(0, |value| value.trim().parse().unwrap());
        while bytes.len() < end + 4 + length {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&buffer[..read]);
        }
        socket.write_all(head.as_bytes()).await.unwrap();
        for part in body.chunks(61) {
            if socket.write_all(part).await.is_err() || socket.flush().await.is_err() {
                break;
            }
        }
    });
    (origin, server)
}

fn streamed() -> String {
    format!("HTTP/1.1 200 OK\r\ncontent-type: {EVENT_STREAM}\r\nconnection: close\r\n\r\n")
}

/// Declares 100 bytes more than `body`, so the connection closes inside it.
fn cut_short(body: &[u8]) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {EVENT_STREAM}\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n",
        body.len() + 100
    )
}

fn target(api: Api, endpoint: &Url) -> BedrockTarget {
    let (protocol, model_id) = match api {
        Api::Converse => (Protocol::BedrockConverse, "us.amazon.nova-micro-v1:0"),
        Api::AnthropicInvoke => (
            Protocol::Messages,
            "anthropic.claude-3-sonnet-20240229-v1:0",
        ),
    };
    let provenance = Provenance {
        protocol,
        provider: Id::new("bedrock").unwrap(),
        account: Id::new("fixture-account").unwrap(),
        endpoint: Id::new("fixture-region").unwrap(),
        model: Id::new("fixture-model").unwrap(),
        binding_revision: Id::new("fixture-1").unwrap(),
    };
    let capabilities = Capabilities::text(200_000, 4_096)
        .with_tools(true)
        .with_tool_choice(true)
        .with_output_streaming(true);
    BedrockTarget::new(
        provenance,
        capabilities,
        "us-east-1",
        model_id,
        api,
        true,
        Some(endpoint.clone()),
    )
    .expect("a valid fixture target")
}

async fn failed(api: Api, head: String, body: Vec<u8>) -> (Error, Vec<StreamEvent>) {
    let (origin, server) = serve(head, body).await;
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(30),
    })
    .unwrap();
    let client = BedrockClient::new(target(api, &origin), http, std::sync::Arc::new(Keys)).unwrap();
    let mut sink = VecSink::new(1_000, 1 << 20);
    let request = TurnRequest::new("fixture-model", vec![Item::user("Hello")]);
    let result = client.turn(&request, &mut sink, &Cancel::new()).await;
    server.await.unwrap();
    (result.expect_err("the attempt fails"), sink.into_events())
}

fn frame(message_type: &str, name_header: &str, name: &str, payload: &Value) -> Vec<u8> {
    encode_message(&Message {
        headers: vec![
            (name_header.to_owned(), HeaderValue::String(name.to_owned())),
            (
                ":content-type".to_owned(),
                HeaderValue::String("application/json".to_owned()),
            ),
            (
                ":message-type".to_owned(),
                HeaderValue::String(message_type.to_owned()),
            ),
        ],
        payload: serde_json::to_vec(payload).unwrap(),
    })
}

fn event(kind: &str, payload: &Value) -> Vec<u8> {
    frame("event", ":event-type", kind, payload)
}

fn chunk(payload: &Value) -> Vec<u8> {
    event("chunk", &json!({"bytes": STANDARD.encode(payload.to_string())}))
}

fn exception(kind: &str) -> Vec<u8> {
    frame(
        "exception",
        ":exception-type",
        kind,
        &json!({"message": "fixture exception text"}),
    )
}

fn opening(api: Api) -> Vec<u8> {
    match api {
        Api::Converse => event("messageStart", &json!({"role": "assistant"})),
        Api::AnthropicInvoke => chunk(&json!({
            "type": "message_start",
            "message": {"id": "msg_fixture", "type": "message", "role": "assistant",
                "content": [], "model": "claude-fixture", "stop_reason": null,
                "stop_sequence": null, "usage": {"input_tokens": 1, "output_tokens": 0}}
        })),
    }
}

/// After the opening, output of which the caller is shown nothing, on `api`, by name.
fn unshown(api: Api) -> Vec<(&'static str, Vec<u8>)> {
    let with = |rest: Vec<u8>| {
        let mut body = opening(api);
        body.extend(rest);
        body
    };
    match api {
        Api::Converse => vec![
            (
                "a redacted reasoning delta",
                with(event(
                    "contentBlockDelta",
                    &json!({"contentBlockIndex": 0,
                        "delta": {"reasoningContent": {"redactedContent": "ZW5jcnlwdGVk"}}}),
                )),
            ),
            (
                "a signature-only reasoning delta",
                with(event(
                    "contentBlockDelta",
                    &json!({"contentBlockIndex": 0,
                        "delta": {"reasoningContent": {"signature": "c2ln"}}}),
                )),
            ),
            (
                "a message stop with no content",
                with(event("messageStop", &json!({"stopReason": "end_turn"}))),
            ),
        ],
        Api::AnthropicInvoke => vec![
            (
                "a redacted_thinking block start",
                with(chunk(&json!({"type": "content_block_start", "index": 0,
                    "content_block": {"type": "redacted_thinking", "data": "ZW5jcnlwdGVk"}}))),
            ),
            (
                "an empty thinking block start and its signature",
                with({
                    let mut rest = chunk(&json!({"type": "content_block_start", "index": 0,
                        "content_block": {"type": "thinking", "thinking": "", "signature": ""}}));
                    rest.extend(chunk(&json!({"type": "content_block_delta", "index": 0,
                        "delta": {"type": "signature_delta", "signature": "c2ln"}})));
                    rest
                }),
            ),
            (
                "a stop reason with no content",
                with(chunk(&json!({"type": "message_delta",
                    "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                    "usage": {"output_tokens": 0}}))),
            ),
        ],
    }
}

/// The unit's "silent" tool block start; the caller is shown a `ToolCallStarted`.
fn tool_start(api: Api) -> Vec<u8> {
    let mut body = opening(api);
    body.extend(match api {
        Api::Converse => event(
            "contentBlockStart",
            &json!({"contentBlockIndex": 0,
                "start": {"toolUse": {"toolUseId": "tool_1", "name": "lookup"}}}),
        ),
        Api::AnthropicInvoke => chunk(&json!({
            "type": "content_block_start", "index": 0,
            "content_block": {"type": "tool_use", "id": "tool_1", "name": "lookup", "input": {}}
        })),
    });
    body
}

const WIRES: [Api; 2] = [Api::Converse, Api::AnthropicInvoke];

#[tokio::test]
async fn throttling_after_output_the_caller_was_not_shown_is_final_on_both_wires() {
    for api in WIRES {
        for (name, body) in unshown(api) {
            let mut body = body;
            body.extend(exception("throttlingException"));
            let (error, events) = failed(api, streamed(), body).await;
            assert_eq!(events, [] as [StreamEvent; 0], "{api:?} {name}: shown nothing");
            assert_eq!(error.code, ErrorCode::RateLimited, "{api:?} {name}");
            assert_eq!(error.dispatch, Dispatch::Accepted, "{api:?} {name}");
            assert!(
                !error.retriable && !error.may_retry(),
                "{api:?} {name}: the model answered, so throttling after it is final"
            );
        }
    }
}

#[tokio::test]
async fn a_cut_after_output_the_caller_was_not_shown_is_final_on_both_wires() {
    for api in WIRES {
        for (name, body) in unshown(api) {
            let (error, events) = failed(api, cut_short(&body), body).await;
            assert_eq!(events, [] as [StreamEvent; 0], "{api:?} {name}: shown nothing");
            assert_eq!(error.code, ErrorCode::Transport, "{api:?} {name}");
            assert!(
                !error.retriable && !error.may_retry(),
                "{api:?} {name}: the model answered, so a cut after it is final"
            );
        }
    }
}

#[tokio::test]
async fn every_transient_exception_is_retriable_before_content_and_final_after_on_both_wires() {
    for api in WIRES {
        for kind in TRANSIENT {
            let mut before = opening(api);
            before.extend(exception(kind));
            let (error, _) = failed(api, streamed(), before).await;
            assert!(error.retriable && error.may_retry(), "{api:?} {kind} before content");

            let mut after = tool_start(api);
            after.extend(exception(kind));
            let (error, _) = failed(api, streamed(), after).await;
            assert!(!error.retriable && !error.may_retry(), "{api:?} {kind} after a tool start");
        }
        let mut stream_error = opening(api);
        stream_error.extend(exception("modelStreamErrorException"));
        let (error, _) = failed(api, streamed(), stream_error).await;
        assert!(
            !error.retriable && !error.may_retry(),
            "{api:?} modelStreamErrorException is final even before content"
        );
    }
}
