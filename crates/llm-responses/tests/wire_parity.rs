//! Harness parity for behaviour llm already has, pinned by name (`docs/harness-parity.md` rows
//! R3, R14, R18, R25, and the default route of R8, R13 and R44).
//!
//! Each case here describes what `spec/domains/responses.yaml` states. They were written before
//! any change to the crate and pass on its base, so each one names the mutation that turns it red.

mod wire_support;

use llm_core::{
    CallId, Cancel, Dispatch, ErrorCode, Item, Model, StopReason, StreamEvent, ToolCall, ToolName,
    ToolSpec, TurnRequest, VecSink,
};
use llm_responses::{decode_stream, ingest_request, project_request};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use wire_support::{
    TEXT_STREAM, body_fixture, canonical_request, client, fixture, listener, projection, serve,
};

const DONE: &[u8] = b"data: [DONE]\n\n";

/// R3. A route that ends its stream with `data: [DONE]` after the terminal object still answers:
/// terminal truth is the response object, and nothing after the first one is read. Mutation: read
/// past the terminal object (drop the `if last { break; }` in `client.rs`) and the sentinel, not
/// being JSON, refuses the turn.
#[tokio::test]
async fn a_stream_ending_with_the_done_sentinel_answers_its_turn() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![[TEXT_STREAM, DONE].concat()]);
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let outcome = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect("a stream that ends with the sentinel after its terminal object is a turn");
    server.await.expect("the fixture server");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Done.")]);
    assert_eq!(sink.text(), "Done.");
}

/// R3, the other side. A sentinel before any terminal object is not JSON on this route's framing,
/// and the turn ends as `protocol` after dispatch rather than as a turn that completed.
#[tokio::test]
async fn a_done_sentinel_before_any_terminal_object_is_refused_as_protocol() {
    let created: &[u8] = b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n";
    let (listener, url) = listener().await;
    let server = serve(listener, vec![[created, DONE].concat()]);
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let error = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect_err("a sentinel is not terminal truth");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Protocol, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
}

/// R14. Every field the turn leaves unset is missing from the bytes the client sends, not present
/// as `null`. Reading the body back cannot tell those apart (ingress reads `null` as absent), so
/// this reads the captured bytes. Mutation: insert `"max_output_tokens": null` for an unset bound
/// in `project_request`.
#[tokio::test]
async fn an_unset_output_bound_is_absent_from_the_body_not_null() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    let request = TurnRequest::new("small", vec![Item::user("Hi")]);
    assert_eq!(request.max_output_tokens, None);
    client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect("a decoded turn");
    let captured = server.await.expect("the fixture server").remove(0);
    let body: Value = serde_json::from_slice(&captured.body).expect("a JSON body");
    let fields: BTreeSet<&str> = body
        .as_object()
        .expect("an object body")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        BTreeSet::from(["include", "input", "model", "store", "stream", "tools"]),
        "{body}"
    );
    let text = String::from_utf8(captured.body).expect("UTF-8 body");
    assert!(!text.contains("max_output_tokens"), "{text}");
    assert!(!text.contains("null"), "{text}");
}

fn tool_turn(output: Value, failed: bool) -> TurnRequest {
    let mut request = TurnRequest::new(
        "small",
        vec![
            Item::user("Read it"),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").expect("call id"),
                name: ToolName::new("file_read").expect("tool name"),
                arguments: json!({"path": "README.md"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call-1").expect("call id"),
                output,
                failed,
            },
        ],
    );
    request.tools = vec![ToolSpec {
        name: ToolName::new("file_read").expect("tool name"),
        description: "Read one file".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request
}

fn result_text(request: &TurnRequest) -> String {
    let body = project_request(&projection("http://127.0.0.1:9/v1"), request).expect("projects");
    body["input"]
        .as_array()
        .and_then(|input| input.last())
        .and_then(|entry| entry["output"].as_str())
        .expect("a function_call_output carries text")
        .to_owned()
}

/// R18, decided: llm keeps one envelope for every result where Harness sends a successful string
/// unenveloped (Harness `project.rs:536`). Mutation: send a successful string result as its own
/// text in `tool_result_envelope`.
#[test]
fn a_successful_string_tool_result_travels_inside_the_envelope() {
    let text = result_text(&tool_turn(json!("plain text"), false));
    assert_eq!(text, r#"{"ok":true,"output":"plain text"}"#);
    assert_ne!(text, "plain text");
}

/// R18, decided: a failure keeps its value under `output`, not `error` as in Harness
/// (`project.rs:42`-`46`). Mutation: write the failure under `error`.
#[test]
fn a_failed_tool_result_keeps_its_value_under_output_not_error() {
    let text = result_text(&tool_turn(json!("not granted"), true));
    assert_eq!(text, r#"{"ok":false,"output":"not granted"}"#);
}

/// R18, the reason for the decision. A successful result whose own text is shaped like Harness's
/// failure envelope, and a successful string that reads as a number, both come back from the body
/// unchanged and still successful. Under Harness's encoding the first would read back as a
/// failure and the second as the number 42.
#[test]
fn a_result_shaped_like_another_encoding_reads_back_unchanged() {
    let binding = projection("http://127.0.0.1:9/v1");
    for output in [json!(r#"{"ok":false,"error":"x"}"#), json!("42")] {
        let request = tool_turn(output.clone(), false);
        let body = project_request(&binding, &request).expect("projects");
        let returned = ingest_request(&binding, &body).expect("reads back");
        assert_eq!(returned, request, "{output}");
    }
}

fn unknown_item() -> Value {
    json!({"type": "web_search_call", "id": "ws_1"})
}

fn warnings(events: &[StreamEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, .. } => Some(code.as_str()),
            _ => None,
        })
        .collect()
}

fn assert_preserved(items: &[Item]) {
    let binding = projection("http://127.0.0.1:9/v1");
    assert_eq!(
        items,
        [Item::Opaque {
            provenance: binding.provenance().clone(),
            payload: unknown_item(),
        }]
    );
}

/// R25, as in Harness `project.rs:628`: an output item outside the subset in the terminal
/// `output` is kept as opaque state of this binding and warned about. Mutation: skip unknown items
/// in `output_item`, or drop the warning.
#[test]
fn an_unknown_output_item_is_preserved_with_a_warning() {
    let decoding = decode_stream(
        &projection("http://127.0.0.1:9/v1"),
        &[json!({
            "type": "response.completed",
            "response": {"status": "completed", "output": [unknown_item()]},
        })],
    );
    assert_eq!(warnings(&decoding.events), ["unknown-output-item"]);
    let outcome = decoding
        .result
        .expect("an unknown item does not refuse the turn");
    assert_preserved(&outcome.items);
}

/// R25, streamed: the same item announced by `response.output_item.done` before an empty terminal
/// `output`, as the Codex backend completes, is kept once and warned about once.
#[test]
fn a_streamed_unknown_output_item_is_preserved_with_a_warning() {
    let decoding = decode_stream(
        &projection("http://127.0.0.1:9/v1"),
        &[
            json!({"type": "response.output_item.done", "output_index": 0, "item": unknown_item()}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ],
    );
    assert_eq!(warnings(&decoding.events), ["unknown-output-item"]);
    let outcome = decoding
        .result
        .expect("an unknown item does not refuse the turn");
    assert_preserved(&outcome.items);
}

/// R8, R13, default route. A client nobody gave a conversation sends no cache key and no identity
/// header, which is what the Codex backend accepted on the 2026-10-04 probe. Mutation: send a
/// `prompt_cache_key` or a `session-id` without a conversation.
#[tokio::test]
async fn a_client_without_a_conversation_sends_no_identity() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url);
    let mut sink = VecSink::new(16, 4096);
    client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hi")]),
            &mut sink,
            &Cancel::new(),
        )
        .await
        .expect("a decoded turn");
    let captured = server.await.expect("the fixture server").remove(0);
    let body: Value = serde_json::from_slice(&captured.body).expect("a JSON body");
    assert!(body.get("prompt_cache_key").is_none(), "{body}");
    let names: BTreeSet<String> = captured
        .headers()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        [
            "accept",
            "authorization",
            "content-length",
            "content-type",
            "host"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>(),
        "{}",
        captured.head
    );
}

/// Ingress does not read a conversation key: the neutral turn has nowhere to carry it, so a body
/// with one is refused rather than read without it. Mutation: add `prompt_cache_key` to
/// `ACCEPTED_BODY_FIELDS`.
#[test]
fn ingress_refuses_a_body_carrying_a_conversation_key() {
    let binding = projection("http://127.0.0.1:9/v1");
    let mut body = project_request(&binding, &TurnRequest::new("small", vec![Item::user("Hi")]))
        .expect("projects");
    body["prompt_cache_key"] = json!("conv-1");
    let error = ingest_request(&binding, &body).expect_err("a conversation key is not read");
    assert_eq!(error.code, ErrorCode::Unsupported, "{error}");
}

/// R44, default route: the request a client without a conversation sends for the canonical turn,
/// body bytes and every header, matches the recorded fixture. Mutation: any change to a body
/// field, its order or its encoding, or to a header name or value.
#[tokio::test]
async fn the_recorded_default_request_matches_its_fixture() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url);
    let request = canonical_request(client.provenance());
    let mut sink = VecSink::new(16, 4096);
    // What the turn answers is not this case's business; the request it sent is.
    let _ = client.turn(&request, &mut sink, &Cancel::new()).await;
    let captured = server.await.expect("the fixture server").remove(0);
    assert_eq!(
        String::from_utf8_lossy(&captured.body),
        String::from_utf8_lossy(&body_fixture("default-request.json")),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
    assert_eq!(
        captured.recorded_head(),
        String::from_utf8(fixture("default-request-head.txt")).expect("UTF-8 fixture")
    );
}
