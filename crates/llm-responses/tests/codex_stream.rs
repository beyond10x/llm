//! A Codex-style answer, served from `127.0.0.1`. No provider is contacted.
//!
//! The fixture reproduces the shape a live probe of the Codex backend recorded on 2026-10-04:
//! `200` over HTTP/1.1 with chunked transfer encoding and **no `content-type` header**; the
//! events `response.created`, `response.in_progress`, `response.output_item.added`, a run of
//! `response.function_call_arguments.delta`, `response.function_call_arguments.done`,
//! `response.output_item.done` carrying the finished `function_call`, and then
//! `response.completed` whose `response.output` is `[]`. Identifiers and text are synthetic.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, Dispatch, ErrorCode, Id, Item,
    Model, Protocol, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const UPSTREAM_MODEL: &str = "example/Small-Model";
const TOOL: &str = "pick_protocol";
const RESPONSE_ID: &str = "resp_fixture_0001";
const ITEM_ID: &str = "fc_fixture_0001";
const CALL_ID: &str = "call_fixture_0001";

/// The response head the probe recorded: no `content-type`, chunked, and headers the client has
/// no reason to read.
const CODEX_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\n\
transfer-encoding: chunked\r\n\
connection: close\r\n\
x-content-type-options: nosniff\r\n\
x-request-id: fixture-request\r\n\r\n";

/// The same head naming a content type that is not an event stream.
const JSON_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\n\
content-type: application/json\r\n\
transfer-encoding: chunked\r\n\
connection: close\r\n\r\n";

/// The arguments of the one function call, in the fragments the deltas stream them in.
const ARGUMENT_DELTAS: &[&str] = &[
    "{\"",
    "confidence\":",
    "0.99,\"protocol\":\"",
    "software-change@1\",\"reasons\":[\"",
    "the request changes code\"]}",
];

fn arguments() -> String {
    ARGUMENT_DELTAS.concat()
}

/// The in-progress response object `response.created` and `response.in_progress` carry.
fn in_progress() -> Value {
    json!({
        "id": RESPONSE_ID,
        "object": "response",
        "status": "in_progress",
        "model": UPSTREAM_MODEL,
        "output": [],
        "usage": null,
    })
}

/// The terminal event as the probe saw it: completed, counters reported, `output` empty.
fn completed() -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": RESPONSE_ID,
            "object": "response",
            "status": "completed",
            "model": UPSTREAM_MODEL,
            "output": [],
            "usage": {
                "input_tokens": 92,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens": 60,
                "output_tokens_details": {"reasoning_tokens": 0},
                "total_tokens": 152,
            },
        },
    })
}

/// The whole Codex-style event sequence, one payload per event, with sequence numbers.
fn codex_events() -> Vec<Value> {
    let mut events = vec![
        json!({"type": "response.created", "response": in_progress()}),
        json!({"type": "response.in_progress", "response": in_progress()}),
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "id": ITEM_ID,
                "type": "function_call",
                "status": "in_progress",
                "arguments": "",
                "call_id": CALL_ID,
                "name": TOOL,
            },
        }),
    ];
    for delta in ARGUMENT_DELTAS {
        events.push(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": ITEM_ID,
            "output_index": 0,
            "delta": delta,
        }));
    }
    events.push(json!({
        "type": "response.function_call_arguments.done",
        "item_id": ITEM_ID,
        "output_index": 0,
        "arguments": arguments(),
    }));
    events.push(json!({
        "type": "response.output_item.done",
        "output_index": 0,
        "item": {
            "id": ITEM_ID,
            "type": "function_call",
            "status": "completed",
            "arguments": arguments(),
            "call_id": CALL_ID,
            "name": TOOL,
        },
    }));
    events.push(completed());
    number(events)
}

/// A stream that streams no output item at all and then completes with `output: []`.
fn empty_events() -> Vec<Value> {
    number(vec![
        json!({"type": "response.created", "response": in_progress()}),
        json!({"type": "response.in_progress", "response": in_progress()}),
        completed(),
    ])
}

fn number(mut events: Vec<Value>) -> Vec<Value> {
    for (sequence, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(sequence);
    }
    events
}

/// Writes `head`, then each event as its own HTTP/1.1 chunk, then the last chunk.
async fn serve(socket: &mut TcpStream, head: &[u8], events: &[Value]) {
    socket.write_all(head).await.expect("head written");
    for event in events {
        let kind = event["type"].as_str().expect("an event type");
        let frame = format!("event: {kind}\ndata: {event}\n\n");
        let chunk = format!("{:x}\r\n{frame}\r\n", frame.len());
        socket
            .write_all(chunk.as_bytes())
            .await
            .expect("chunk written");
    }
    socket.write_all(b"0\r\n\r\n").await.expect("last chunk");
    socket.shutdown().await.expect("closed");
}

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
    }
}

/// A bearer-authenticated Responses binding at `base_url`.
fn binding(base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Subscription,
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id(UPSTREAM_MODEL),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: capabilities(),
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

/// An injected resolver holding one fixture credential.
struct StaticResolver;

impl SecretResolver for StaticResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

fn client(url: &str) -> ResponsesClient {
    let total = Duration::from_secs(10);
    ResponsesClient::new(
        binding(url),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver),
    )
    .expect("a Responses binding")
}

/// A turn that forces one named tool, as the probe's request did.
fn request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Pick the protocol")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new(TOOL).expect("tool name"),
        description: "Choose the work protocol for the request.".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request.tool_choice = ToolChoice::Named(ToolName::new(TOOL).expect("tool name"));
    request
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!(
        "http://{}/backend-api/codex",
        listener.local_addr().expect("an address")
    );
    (listener, url)
}

/// Bounded so that a client which never connects fails the test instead of hanging it.
async fn accept(listener: &TcpListener) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never opened a connection")
        .expect("an accepted connection")
        .0
}

/// Reads one whole request: its head and the body its `content-length` declares.
async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

/// Serves one answer with `head` and `events` and returns what the client asked.
fn fixture(
    listener: TcpListener,
    head: &'static [u8],
    events: Vec<Value>,
) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        let captured = read_request(&mut socket).await;
        serve(&mut socket, head, &events).await;
        captured
    })
}

/// The acceptance case: no `content-type`, the call streamed through `output_item.done`, and a
/// terminal `output` of `[]`. The turn returns the streamed call with its arguments.
#[tokio::test]
async fn a_codex_style_stream_completes_its_turn() {
    let (listener, url) = listener().await;
    let server = fixture(listener, CODEX_HEAD, codex_events());
    let mut sink = VecSink::new(32, 4096);
    let outcome = client(&url)
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a Codex-style stream completes its turn");
    let captured = server.await.expect("the fixture server");

    // The request asked for an event stream, which is what lets a missing content type be read
    // as one.
    let head = captured
        .split_once("\r\n\r\n")
        .expect("a head and a body")
        .0
        .to_ascii_lowercase();
    assert!(
        head.starts_with("post /backend-api/codex/responses "),
        "{head}"
    );
    assert!(
        head.lines().any(|line| line == "accept: text/event-stream"),
        "{head}"
    );

    let call_id = CallId::new(CALL_ID).expect("call id");
    let name = ToolName::new(TOOL).expect("tool name");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.tool_calls().cloned().collect::<Vec<_>>(),
        vec![ToolCall {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: json!({
                "confidence": 0.99,
                "protocol": "software-change@1",
                "reasons": ["the request changes code"],
            }),
        }]
    );
    assert_eq!(outcome.items.len(), 1, "{:?}", outcome.items);

    // Final usage, from the terminal object.
    let observation = &outcome.observation;
    assert!(observation.final_usage);
    assert_eq!(observation.response_id, Some(id(RESPONSE_ID)));
    let usage = observation.usage.as_ref().expect("reported usage");
    assert_eq!(usage.input_tokens, Some(92));
    assert_eq!(usage.output_tokens, Some(60));
    assert_eq!(usage.cached_input_tokens, Some(0));
    assert_eq!(usage.reasoning_output_tokens, Some(0));

    // The caller saw the announcement and every argument fragment, in order.
    let mut expected = vec![StreamEvent::ToolCallStarted {
        call_id: call_id.clone(),
        name,
    }];
    expected.extend(
        ARGUMENT_DELTAS
            .iter()
            .map(|delta| StreamEvent::ToolArgumentsDelta {
                call_id: call_id.clone(),
                delta: (*delta).to_owned(),
            }),
    );
    assert_eq!(sink.events(), expected);
}

/// A `2xx` that names a content type other than an event stream is still refused, before any
/// event is decoded, even though the body it carries would decode to a turn.
#[tokio::test]
async fn a_success_naming_another_content_type_is_still_refused() {
    let (listener, url) = listener().await;
    let server = fixture(listener, JSON_HEAD, codex_events());
    let mut sink = VecSink::new(32, 4096);
    let error = client(&url)
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("a success that names another content type is not an event stream");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert_eq!(
        error.message, "HTTP success response is not text/event-stream",
        "{error}"
    );
    assert!(sink.events().is_empty(), "{:?}", sink.events());
}

/// An empty terminal `output` with no streamed item is still refused: the fallback to streamed
/// items has nothing to fall back to, and a forced tool that was never called is no turn.
///
/// The refusal comes after the terminal object was read, so it carries the reported counters;
/// that is what separates it from a refusal of the response head.
#[tokio::test]
async fn an_empty_terminal_output_with_no_streamed_items_is_still_refused() {
    let (listener, url) = listener().await;
    let server = fixture(listener, CODEX_HEAD, empty_events());
    let mut sink = VecSink::new(32, 4096);
    let error = client(&url)
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("a turn without output is refused");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert_eq!(
        error.message, "model terminal reason contradicts its tool obligations",
        "{error}"
    );
    let observation = error.observation.as_deref().expect("terminal evidence");
    assert!(observation.final_usage, "{observation:?}");
    assert_eq!(
        observation
            .usage
            .as_ref()
            .and_then(|usage| usage.input_tokens),
        Some(92),
        "{observation:?}"
    );
    assert!(sink.events().is_empty(), "{:?}", sink.events());
}
