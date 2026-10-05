//! Adversarial cases for the Codex-stream fallback: an empty terminal `output` after streamed
//! items, and a `2xx` with no `content-type`. No provider is contacted; every answer is a local
//! fixture.
//!
//! Each case names the claim it drives: `docs/responses.md` ("Terminal truth and failure"), the
//! story's Outcome ("a non-empty terminal `output` keeps today's rule") or its Acceptance ("an
//! empty terminal `output` with no streamed items is still refused as a turn without output").

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, Dispatch, ErrorCode, Id, Item,
    Model, Protocol, Provenance, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{Binding, ResponsesClient, decode_stream};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const UPSTREAM_MODEL: &str = "example/Small-Model";
const TOOL: &str = "pick_protocol";
const OTHER_TOOL: &str = "file_read";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: id("my-lab"),
        account: id("local"),
        endpoint: id("local-endpoint"),
        model: id("small"),
        binding_revision: id("rev-1"),
    }
}

fn binding() -> Binding {
    Binding::new(provenance(), id(UPSTREAM_MODEL))
}

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).expect("tool name"),
        description: "A fixture tool.".to_owned(),
        input_schema: json!({"type": "object"}),
    }
}

/// Two published tools, the model free to call either, both or none.
fn auto_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Pick the protocol")]);
    request.tools = vec![tool(TOOL), tool(OTHER_TOOL)];
    request
}

/// One published tool the model must call, as the probe's request did.
fn forced_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Pick the protocol")]);
    request.tools = vec![tool(TOOL)];
    request.tool_choice = ToolChoice::Named(ToolName::new(TOOL).expect("tool name"));
    request
}

fn usage() -> Value {
    json!({
        "input_tokens": 92,
        "input_tokens_details": {"cached_tokens": 0},
        "output_tokens": 60,
        "output_tokens_details": {"reasoning_tokens": 12},
        "total_tokens": 152,
    })
}

/// A terminal event of `kind` carrying `output` and the fixture counters.
fn terminal(kind: &str, output: &Value) -> Value {
    let mut response = json!({
        "id": "resp_fixture_0001",
        "object": "response",
        "status": if kind == "response.incomplete" { "incomplete" } else { "completed" },
        "model": UPSTREAM_MODEL,
        "output": output,
        "usage": usage(),
    });
    if kind == "response.incomplete" {
        response["incomplete_details"] = json!({"reason": "max_output_tokens"});
    }
    json!({"type": kind, "response": response})
}

fn completed_empty() -> Value {
    terminal("response.completed", &json!([]))
}

fn call_item(item_id: &str, call_id: &str, name: &str, arguments: &str) -> Value {
    json!({
        "id": item_id,
        "type": "function_call",
        "status": "completed",
        "arguments": arguments,
        "call_id": call_id,
        "name": name,
    })
}

fn added(item: &Value) -> Value {
    let mut opening = item.clone();
    opening["status"] = json!("in_progress");
    opening["arguments"] = json!("");
    json!({"type": "response.output_item.added", "output_index": 0, "item": opening})
}

fn done(item: &Value) -> Value {
    json!({"type": "response.output_item.done", "output_index": 0, "item": item})
}

fn message_item(text: &str) -> Value {
    json!({
        "id": "msg_fixture_0001",
        "type": "message",
        "role": "assistant",
        "status": "completed",
        "content": [{"type": "output_text", "text": text, "annotations": []}],
    })
}

fn reasoning_item() -> Value {
    json!({
        "id": "rs_fixture_0001",
        "type": "reasoning",
        "summary": [],
        "encrypted_content": "c3ludGhldGljLWVuY3J5cHRlZC1zdGF0ZQ==",
    })
}

fn tool_call(call_id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(call_id).expect("call id"),
        name: ToolName::new(name).expect("tool name"),
        arguments,
    })
}

// ---------------------------------------------------------------------------------------------
// The decoder: a non-empty terminal `output` keeps today's rule.
// ---------------------------------------------------------------------------------------------

/// Story Outcome: "a non-empty terminal `output` keeps today's rule" — the terminal object is
/// authoritative when it carries output, whatever was streamed before it.
#[test]
fn a_non_empty_terminal_output_still_outranks_the_streamed_items() {
    let payloads = vec![
        done(&message_item("a draft the server later replaced")),
        terminal(
            "response.completed",
            &json!([message_item("the final answer")]),
        ),
    ];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("a completed text turn");
    assert_eq!(outcome.items, vec![Item::assistant("the final answer")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.validate_for(&auto_request(), &provenance()), Ok(()));
}

/// Story Outcome, the same rule from the refusing side: a non-empty terminal `output` that
/// omits a call the caller was shown is still "a finished function call contradicts its
/// announcement", and the refusal still carries the counters and passes `Error::validate_for`.
#[test]
fn a_non_empty_terminal_output_that_omits_a_streamed_call_is_still_refused() {
    let call = call_item("fc_1", "call_1", TOOL, "{\"protocol\":\"x\"}");
    let payloads = vec![
        added(&call),
        done(&call),
        terminal("response.completed", &json!([message_item("no call here")])),
    ];
    let error = decode_stream(&binding(), &payloads)
        .result
        .expect_err("the terminal output dropped an announced call");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert_eq!(
        error.message, "a finished function call contradicts its announcement",
        "{error}"
    );
    let observation = error.observation.as_deref().expect("terminal evidence");
    assert!(observation.final_usage, "{observation:?}");
    assert_eq!(error.validate_for(&provenance()), Ok(()));
}

// ---------------------------------------------------------------------------------------------
// The decoder: the fallback to streamed items.
// ---------------------------------------------------------------------------------------------

/// The probe's request asked for `reasoning.encrypted_content`. A Codex turn that reasons streams
/// the reasoning item before the call; under the fallback it is carried exactly once, ahead of
/// the call, with the terminal counters and a `ToolCalls` stop.
#[test]
fn the_fallback_keeps_a_streamed_reasoning_item_once_beside_the_call() {
    let call = call_item("fc_1", "call_1", TOOL, "{\"protocol\":\"x\"}");
    let payloads = vec![
        json!({"type": "response.output_item.added", "output_index": 0, "item": {
            "id": "rs_fixture_0001", "type": "reasoning", "summary": []}}),
        done(&reasoning_item()),
        added(&call),
        done(&call),
        completed_empty(),
    ];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("a Codex-style reasoning-then-call turn");
    assert_eq!(
        outcome.items,
        vec![
            Item::Opaque {
                provenance: provenance(),
                payload: reasoning_item(),
            },
            tool_call("call_1", TOOL, json!({"protocol": "x"})),
        ]
    );
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert!(outcome.observation.final_usage);
    let usage = outcome.observation.usage.as_ref().expect("counters");
    assert_eq!(usage.reasoning_output_tokens, Some(12));
    assert_eq!(
        outcome.validate_for(&forced_request(), &provenance()),
        Ok(())
    );
}

/// Every streamed call survives the fallback, in the order the wire finished them.
#[test]
fn the_fallback_keeps_every_streamed_call_in_wire_order() {
    let first = call_item("fc_1", "call_1", TOOL, "{\"protocol\":\"x\"}");
    let second = call_item("fc_2", "call_2", OTHER_TOOL, "{\"path\":\"README.md\"}");
    let payloads = vec![
        added(&first),
        added(&second),
        done(&first),
        done(&second),
        completed_empty(),
    ];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("two streamed calls");
    assert_eq!(
        outcome.items,
        vec![
            tool_call("call_1", TOOL, json!({"protocol": "x"})),
            tool_call("call_2", OTHER_TOOL, json!({"path": "README.md"})),
        ]
    );
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(outcome.validate_for(&auto_request(), &provenance()), Ok(()));
}

/// The fallback does not license a call the caller was shown and never received: one announced
/// call that never reached `output_item.done` still refuses the turn, with its counters.
#[test]
fn an_announced_call_that_never_finished_is_refused_under_the_fallback() {
    let finished = call_item("fc_1", "call_1", TOOL, "{\"protocol\":\"x\"}");
    let unfinished = call_item("fc_2", "call_2", OTHER_TOOL, "{}");
    let payloads = vec![
        added(&finished),
        added(&unfinished),
        done(&finished),
        completed_empty(),
    ];
    let error = decode_stream(&binding(), &payloads)
        .result
        .expect_err("an announced call is missing from the turn");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(
        error.message, "a finished function call contradicts its announcement",
        "{error}"
    );
    assert!(
        error
            .observation
            .as_deref()
            .is_some_and(|observation| observation.final_usage),
        "{error:?}"
    );
    assert_eq!(error.validate_for(&provenance()), Ok(()));
}

/// `response.incomplete` with an empty `output` after a finished message: the message the
/// stream finished is the turn, and the stop reason is still the incomplete one.
#[test]
fn an_incomplete_response_with_empty_output_keeps_its_streamed_items() {
    let payloads = vec![
        done(&message_item("a finished paragraph")),
        terminal("response.incomplete", &json!([])),
    ];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("an incomplete turn");
    assert_eq!(outcome.items, vec![Item::assistant("a finished paragraph")]);
    assert_eq!(outcome.stop_reason, StopReason::MaxOutputTokens);
    assert!(outcome.observation.final_usage);
    assert_eq!(outcome.validate_for(&auto_request(), &provenance()), Ok(()));
}

/// Streamed items that disagree with each other: the same call finished twice. Whichever layer
/// says so, the turn is refused, and the refusal passes `Error::validate_for`.
#[test]
fn a_call_streamed_twice_does_not_become_a_turn() {
    let call = call_item("fc_1", "call_1", TOOL, "{\"protocol\":\"x\"}");
    let payloads = vec![added(&call), done(&call), done(&call), completed_empty()];
    let request = forced_request();
    let refused = match decode_stream(&binding(), &payloads).result {
        Err(error) => error,
        Ok(outcome) => match outcome.validate_for(&request, &provenance()) {
            Err(error) => error,
            Ok(()) => panic!("a call finished twice became a turn: {:?}", outcome.items),
        },
    };
    assert_eq!(refused.code, ErrorCode::Protocol, "{refused}");
    assert_eq!(refused.validate_for(&provenance()), Ok(()));
}

/// The caller was shown text (`output_text.delta`) and the server then completed with an empty
/// `output` and no `output_item.done`. The decoder keeps the "the caller already saw it" rule
/// for calls (`announced`), and `spec/domains/responses.yaml` ("Kept text", row R36) extends it
/// to text: the answer the caller watched stream is in the turn it gets back.
#[test]
fn text_the_caller_was_shown_is_not_dropped_from_the_turn() {
    let payloads = vec![
        json!({"type": "response.output_text.delta", "item_id": "msg_fixture_0001",
            "output_index": 0, "content_index": 0, "delta": "The answer is 42."}),
        json!({"type": "response.output_text.done", "item_id": "msg_fixture_0001",
            "output_index": 0, "content_index": 0, "text": "The answer is 42."}),
        completed_empty(),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        decoding.events,
        vec![StreamEvent::TextDelta {
            text: "The answer is 42.".to_owned()
        }]
    );
    if let Ok(outcome) = decoding.result {
        assert!(
            outcome
                .items
                .contains(&Item::assistant("The answer is 42.")),
            "the caller was shown text the turn does not carry: {:?} {:?}",
            outcome.stop_reason,
            outcome.items
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The client, against an untyped `2xx` like the Codex backend's.
// ---------------------------------------------------------------------------------------------

const UNTYPED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\n\
transfer-encoding: chunked\r\n\
connection: close\r\n\r\n";

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

fn provider_binding(base_url: &str) -> llm_providers::Binding {
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
        provider_binding(url),
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

async fn read_request(socket: &mut TcpStream) {
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
                return;
            }
        }
    }
}

/// Serves one untyped `200` whose body is `body`, in one chunk, and returns the client URL.
async fn untyped_fixture(body: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!(
        "http://{}/backend-api/codex",
        listener.local_addr().expect("an address")
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("the client never opened a connection")
            .expect("an accepted connection");
        read_request(&mut socket).await;
        socket.write_all(UNTYPED_HEAD).await.expect("head written");
        let chunk = format!("{:x}\r\n", body.len());
        socket
            .write_all(chunk.as_bytes())
            .await
            .expect("chunk size");
        socket.write_all(&body).await.expect("chunk body");
        socket
            .write_all(b"\r\n0\r\n\r\n")
            .await
            .expect("last chunk");
        let _ = socket.shutdown().await;
    });
    (url, server)
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut body = Vec::new();
    for (sequence, event) in events.iter().enumerate() {
        let mut event = event.clone();
        event["sequence_number"] = json!(sequence);
        let kind = event["type"].as_str().expect("an event type").to_owned();
        body.extend_from_slice(format!("event: {kind}\ndata: {event}\n\n").as_bytes());
    }
    body
}

/// Story Acceptance: "an empty terminal `output` with no streamed items is still refused as a
/// turn without output". The phase-1 case proves it only for a forced tool, where
/// `TurnOutcome::validate_for` refuses for a different reason (the tool obligation). A request
/// whose `tool_choice` is `auto`, answered the Codex way with nothing in it, is accepted as an
/// `EndTurn` turn with no items.
#[tokio::test]
#[ignore = "declined in wave 2026-10-04-w18: an empty auto turn stays an empty EndTurn turn as docs/responses.md states; the story acceptance wording is corrected"]
async fn an_empty_untyped_turn_without_a_forced_tool_is_still_refused() {
    let in_progress = json!({"id": "resp_fixture_0001", "object": "response",
        "status": "in_progress", "model": UPSTREAM_MODEL, "output": [], "usage": null});
    let body = sse(&[
        json!({"type": "response.created", "response": in_progress}),
        json!({"type": "response.in_progress", "response": in_progress}),
        completed_empty(),
    ]);
    let (url, server) = untyped_fixture(body).await;
    let model = client(&url);
    let mut sink = VecSink::new(32, 4096);
    let returned = model.turn(&auto_request(), &mut sink, &Cancel::new()).await;
    server.await.expect("the fixture server");
    let error = returned.expect_err("a turn without output is refused");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.validate_for(model.provenance()), Ok(()));
}

/// An untyped `200` whose body is JSON, not an event stream: it is now read as an event stream
/// (the request asked for one), finds no event and ends. The refusal is the stream's own, it
/// passes `Error::validate_for`, and its dispatch is the one `docs/responses.md` gives a stream
/// that ends without a terminal object (`unknown`), no longer the `accepted` of a refused head.
#[tokio::test]
async fn an_untyped_success_carrying_json_is_refused_as_a_stream_without_a_terminal() {
    let (url, server) =
        untyped_fixture(b"{\"error\":{\"message\":\"synthetic\"}}\n".to_vec()).await;
    let model = client(&url);
    let mut sink = VecSink::new(32, 4096);
    let returned = model.turn(&auto_request(), &mut sink, &Cancel::new()).await;
    server.await.expect("the fixture server");
    let error = returned.expect_err("a JSON body is no turn");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(
        error.message, "the stream ended before the response reached a terminal state",
        "{error}"
    );
    assert_eq!(error.dispatch, Dispatch::Unknown, "{error}");
    assert_eq!(error.validate_for(model.provenance()), Ok(()));
    assert!(sink.events().is_empty(), "{:?}", sink.events());
}
