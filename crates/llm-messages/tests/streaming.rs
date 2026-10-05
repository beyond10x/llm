//! Messages response and event-stream decoding: terminal truth, usage and refusals.
mod support;

use llm_core::{
    Cancel, Dispatch, Error, ErrorCode, Item, MAX_ITEMS, StopReason, StreamEvent, ToolName,
    ToolSpec, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_messages::{decode_message, decode_stream};
use serde_json::{Value, json};
use support::binding;

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: "Look up a record".to_owned(),
        input_schema: json!({"type":"object"}),
    }];
    request
}

/// Frames producer-shaped events exactly as the route sends them: named event, JSON payload.
fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

fn message_start(usage: Value) -> Value {
    let mut message = json!({"id":"msg_014a","type":"message","role":"assistant",
        "model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null});
    message["usage"] = usage;
    json!({"type":"message_start","message":message})
}

fn message_delta(stop_reason: Value, usage: Value) -> Value {
    let mut event = json!({"type":"message_delta","delta":{"stop_sequence":null}});
    event["delta"]["stop_reason"] = stop_reason;
    event["usage"] = usage;
    event
}

fn text_turn(usage_start: Value, usage_end: Value) -> Vec<Value> {
    vec![
        message_start(usage_start),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("end_turn"), usage_end),
        json!({"type":"message_stop"}),
    ]
}

async fn decode(events: &[Value]) -> (Result<TurnOutcome, Error>, VecSink) {
    decode_bytes(&sse(events)).await
}

async fn decode_bytes(bytes: &[u8]) -> (Result<TurnOutcome, Error>, VecSink) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        bytes,
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (outcome, sink)
}

#[tokio::test]
async fn a_streamed_turn_preserves_text_thinking_tools_and_terminal_usage() {
    let events = vec![
        message_start(json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"weighing"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"sig-1"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Looking"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"input_json_delta","partial_json":"\"errors\"}"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"ping"}),
        message_delta(
            json!("tool_use"),
            json!({"input_tokens":11,"cache_read_input_tokens":4,
                "cache_creation_input_tokens":6,"output_tokens":9,
                "output_tokens_details":{"thinking_tokens":3}}),
        ),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let outcome = outcome.expect("decoded");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.items,
        vec![
            Item::Opaque {
                provenance: binding().provenance().clone(),
                payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
            },
            Item::assistant("Looking"),
            Item::ToolCall(llm_core::ToolCall {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"query":"errors"}),
            }),
        ]
    );
    assert_eq!(
        outcome
            .observation
            .upstream_model
            .as_ref()
            .map(ToString::to_string),
        Some("example-model-20260201".to_owned())
    );
    assert_eq!(
        outcome
            .observation
            .response_id
            .as_ref()
            .map(ToString::to_string),
        Some("msg_014a".to_owned())
    );
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage,
        Some(Usage {
            // 11 fresh + 4 read + 6 written; the neutral total is inclusive.
            input_tokens: Some(21),
            output_tokens: Some(9),
            cached_input_tokens: Some(4),
            cache_creation_input_tokens: Some(6),
            reasoning_output_tokens: Some(3),
        })
    );
    assert_eq!(
        sink.events(),
        [
            StreamEvent::ReasoningDelta {
                text: "weighing".to_owned()
            },
            StreamEvent::TextDelta {
                text: "Looking".to_owned()
            },
            StreamEvent::ToolCallStarted {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                delta: "{\"query\":".to_owned()
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: llm_core::CallId::new("call-1").unwrap(),
                delta: "\"errors\"}".to_owned()
            },
        ]
    );
}

/// The thinking block exactly as Harness's emulator opens it (`crates/harness-cli/tests/fixtures/
/// fake_messages.py` `thinking_events`, and Harness's pinned `anthropic-messages/2026-08-29`
/// through `2026-08-30.1` stream fixtures): no `signature` field on `content_block_start`, the
/// signature arriving whole in a `signature_delta`. The finished block is the same one a start
/// carrying `"signature": ""` produces (`docs/harness-parity.md` row M47).
#[tokio::test]
async fn a_thinking_block_that_opens_without_a_signature_field_takes_it_from_its_delta() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"OPAQUE-REASONING-BLOB"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"OPAQUE-SIGNATURE"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":1}),
        message_delta(json!("tool_use"), json!({"output_tokens":9})),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let outcome = outcome.expect("a block opened without a signature field is signed by its delta");
    assert_eq!(
        outcome.items[0],
        Item::Opaque {
            provenance: binding().provenance().clone(),
            payload: json!({"type":"thinking","thinking":"OPAQUE-REASONING-BLOB",
                "signature":"OPAQUE-SIGNATURE"}),
        }
    );
    assert_eq!(
        sink.events()[0],
        StreamEvent::ReasoningDelta {
            text: "OPAQUE-REASONING-BLOB".to_owned()
        }
    );
}

/// The other half of the same rule: a block that opened without a signature field and never
/// received one is still unsigned thinking, refused rather than replayed.
#[tokio::test]
async fn a_thinking_block_that_never_receives_a_signature_is_still_refused() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"weighing"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Done"}}),
        json!({"type":"content_block_stop","index":1}),
        message_delta(json!("end_turn"), json!({"output_tokens":9})),
        json!({"type":"message_stop"}),
    ];
    let error = decode(&events)
        .await
        .0
        .expect_err("a thinking block with no signature at all");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
}

/// Only a thinking block begins a signature it was opened without: a `signature_delta` into a
/// text block still does not extend its block.
#[tokio::test]
async fn a_signature_delta_into_a_text_block_is_still_refused() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"OPAQUE-SIGNATURE"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("end_turn"), json!({"output_tokens":9})),
        json!({"type":"message_stop"}),
    ];
    let error = decode(&events)
        .await
        .0
        .expect_err("a signature delta against a text block");
    assert_eq!(error.code, ErrorCode::Protocol, "{error:?}");
    assert_eq!(error.message, "Messages delta does not extend its block");
}

#[tokio::test]
async fn a_tool_call_is_announced_when_its_block_opens_even_before_any_argument() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("tool_use"), json!({"output_tokens":3})),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    outcome.expect("decoded");
    assert_eq!(
        sink.events(),
        [StreamEvent::ToolCallStarted {
            call_id: llm_core::CallId::new("call-1").unwrap(),
            name: ToolName::new("lookup").unwrap(),
        }]
    );
}

#[tokio::test]
async fn an_opening_block_with_an_unpublishable_name_is_refused_before_it_is_announced() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"call-1","name":"look.up","input":{}}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":0}),
        message_delta(json!("tool_use"), json!({"output_tokens":3})),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Unsupported);
    assert!(sink.events().is_empty(), "{:?}", sink.events());
}

#[tokio::test]
async fn cumulative_usage_replaces_reported_counters_instead_of_summing_deltas() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":8}),
    ))
    .await;
    let usage = outcome.expect("decoded").observation.usage.expect("usage");
    // Summing the two reports would bill 9 output tokens for 8.
    assert_eq!(usage.output_tokens, Some(8));
    assert_eq!(usage.input_tokens, Some(11));
}

#[tokio::test]
async fn a_counter_the_route_never_reported_stays_unknown() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    ))
    .await;
    let usage = outcome.expect("decoded").observation.usage.expect("usage");
    assert_eq!(usage.cached_input_tokens, None);
    assert_eq!(usage.cache_creation_input_tokens, None);
    assert_eq!(usage.reasoning_output_tokens, None);
    // The disjoint total is unknown while a component is: it never collapses to the fresh count.
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.output_tokens, Some(8));
}

#[tokio::test]
async fn a_route_that_reports_no_usage_at_all_reports_none() {
    let events = vec![
        json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
            "role":"assistant","model":"example-model-20260201","content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}),
        json!({"type":"message_stop"}),
    ];
    let outcome = decode(&events).await.0.expect("decoded");
    assert_eq!(outcome.observation.usage, None);
    assert!(outcome.observation.final_usage);
}

#[tokio::test]
async fn a_usage_counter_that_regresses_is_refused() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":8}),
        json!({"output_tokens":3}),
    ))
    .await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn one_hour_cache_creation_is_refused_rather_than_priced_as_five_minute() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":6,
            "output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":6,"ephemeral_5m_input_tokens":0}}),
    ))
    .await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Unsupported);
}

#[tokio::test]
async fn a_five_minute_cache_breakdown_that_contradicts_its_total_is_refused() {
    let (outcome, _) = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":6,
            "output_tokens":8,
            "cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":2}}),
    ))
    .await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn a_charged_server_tool_use_is_refused_and_an_unused_one_is_not() {
    let charged = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,
            "output_tokens":8,"server_tool_use":{"web_search_requests":2}}),
    ))
    .await
    .0;
    assert_eq!(charged.expect_err("refused").code, ErrorCode::Unsupported);
    let unused = decode(&text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":0,
            "cache_creation_input_tokens":0,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,
            "output_tokens":8,"server_tool_use":{"web_search_requests":0}}),
    ))
    .await
    .0;
    assert!(unused.is_ok(), "an unused server tool is not a charge");
}

#[tokio::test]
async fn a_stream_that_ends_without_its_terminal_event_keeps_the_last_usage_snapshot() {
    let mut events = text_turn(
        json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":1}),
        json!({"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":8}),
    );
    events.pop().expect("message_stop");
    let (outcome, sink) = decode(&events).await;
    let error = outcome.expect_err("EOF never manufactures success");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    let observation = error.observation.expect("the snapshot is retained");
    assert!(!observation.final_usage);
    assert_eq!(
        observation.usage.expect("partial usage").output_tokens,
        Some(8)
    );
    assert_eq!(observation.binding, *binding().provenance());
    // The prefix the caller already saw is not withdrawn.
    assert_eq!(
        sink.events(),
        [StreamEvent::TextDelta {
            text: "Hello".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_stream_cut_inside_an_event_is_refused() {
    let mut bytes = sse(&[message_start(json!({"input_tokens":11,"output_tokens":1}))]);
    bytes.extend_from_slice(b"event: content_block_start\ndata: {\"type\":\"content");
    let (outcome, _) = decode_bytes(&bytes).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn an_event_name_that_contradicts_its_payload_type_is_refused() {
    // A ping alone is accepted, so the contradicting name is the only reason to refuse.
    let bytes = b"event: content_block_start\ndata: {\"type\":\"ping\"}\n\n".to_vec();
    let (outcome, _) = decode_bytes(&bytes).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn out_of_order_and_duplicate_transitions_are_refused() {
    let start = message_start(json!({"input_tokens":11,"output_tokens":1}));
    let cases: Vec<Vec<Value>> = vec![
        // A second message_start.
        vec![start.clone(), start.clone()],
        // Content before the message started.
        vec![json!({"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":""}})],
        // A delta for a block that never started.
        vec![
            start.clone(),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":"Hello"}}),
        ],
        // Two blocks open at the same index.
        vec![
            start.clone(),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
        ],
        // A block stop for a block that never started.
        vec![
            start.clone(),
            json!({"type":"content_block_stop","index":3}),
        ],
        // A terminal message leaving a block open.
        vec![
            start.clone(),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
            message_delta(json!("end_turn"), json!({"output_tokens":8})),
            json!({"type":"message_stop"}),
        ],
        // A payload after the terminal event.
        vec![
            start.clone(),
            message_delta(json!("end_turn"), json!({"output_tokens":8})),
            json!({"type":"message_stop"}),
            json!({"type":"ping"}),
        ],
    ];
    for events in cases {
        let (outcome, _) = decode(&events).await;
        let error = outcome.expect_err("refused");
        assert_eq!(error.code, ErrorCode::Protocol, "{events:?}");
    }
}

fn opaque(payload: Value) -> Item {
    Item::Opaque {
        provenance: binding().provenance().clone(),
        payload,
    }
}

fn warning(code: &str, message: &str) -> StreamEvent {
    StreamEvent::Warning {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// `text_turn` with `extra` spliced in at `at`.
fn text_turn_with(at: usize, extra: &[Value]) -> Vec<Value> {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"input_tokens":11,"output_tokens":8}),
    );
    events.splice(at..at, extra.iter().cloned());
    events
}

/// Harness parity M36 (`harness-messages/src/lib.rs:443`-`455`, test `:1146`).
///
/// A route that adds an event type is not a broken stream. The event is kept whole as opaque
/// state bound to the serving binding, the caller is told it was not interpreted, and the turn
/// still completes on its own terminal event. The diagnostic is fixed: no producer text in it.
#[tokio::test]
async fn an_unknown_stream_event_is_preserved_with_a_warning_instead_of_ending_the_turn() {
    let event = json!({"type":"message_reconsidered","detail":"kept"});
    // After the text block stopped, before the terminal delta.
    let (outcome, sink) = decode(&text_turn_with(4, std::slice::from_ref(&event))).await;
    let outcome = outcome.expect("the turn completes");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello"), opaque(event)]);
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            warning(
                "unknown-stream-event",
                "a stream event outside the pinned subset was preserved, not interpreted"
            ),
        ]
    );
    assert!(outcome.observation.final_usage);
}

/// Harness parity M36 (`harness-messages/src/lib.rs:566`-`578`).
///
/// A delta type this subset does not model, inside a block it does, is kept as the whole event and
/// warned about; the block itself still assembles from the deltas around it. The preserved event
/// sits after every block that had started when it arrived.
#[tokio::test]
async fn an_unknown_content_delta_is_preserved_with_a_warning_and_its_block_completes() {
    let event = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"citations_delta","citation":{"cited_text":"kept"}}});
    // Between the text delta and the block stop.
    let (outcome, sink) = decode(&text_turn_with(3, std::slice::from_ref(&event))).await;
    let outcome = outcome.expect("the turn completes");
    assert_eq!(outcome.items, vec![Item::assistant("Hello"), opaque(event)]);
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            warning(
                "unknown-stream-event",
                "a content block delta outside the pinned subset was preserved, not interpreted"
            ),
        ]
    );
}

/// Harness parity M23 (`harness-messages/src/project.rs:321`-`354`, test `:773`).
///
/// A content block type this subset does not model is kept at its index as opaque state bound to
/// the serving binding, and warned about when it opens. It is never dropped: a dropped block is a
/// hole in the conversation the next turn cannot see.
#[tokio::test]
async fn an_unknown_content_block_is_preserved_with_a_warning_instead_of_ending_the_turn() {
    let block = json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search",
        "input":{"query":"errors"}});
    let (outcome, sink) = decode(&text_turn_with(
        4,
        &[
            json!({"type":"content_block_start","index":1,"content_block":block}),
            json!({"type":"content_block_stop","index":1}),
        ],
    ))
    .await;
    let outcome = outcome.expect("the turn completes");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello"), opaque(block)]);
    assert_eq!(
        sink.events(),
        [
            StreamEvent::TextDelta {
                text: "Hello".to_owned()
            },
            warning(
                "unknown-output-item",
                "a content block outside the pinned subset was preserved, not interpreted"
            ),
        ]
    );
}

/// The other half of the M23 decision. A complete response has no stream to warn on, and a block
/// preserved without telling anyone is one nobody knows is uninterpreted, so `decode_message`
/// keeps refusing it.
#[test]
fn a_complete_message_still_refuses_an_unknown_block_it_has_no_channel_to_warn_about() {
    let message = json!({
        "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
        "content":[{"type":"text","text":"Hello"},{"type":"novel_block","detail":"kept"}],
        "stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":8}});
    let error = decode_message(&message, &request(), binding().provenance()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages content is outside the declared subset"
    );
}

#[tokio::test]
async fn a_terminal_message_without_a_stop_reason_is_refused() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[4] = message_delta(Value::Null, json!({"output_tokens":8}));
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn a_terminal_reason_that_contradicts_the_content_is_refused() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{}"}});
    // `end_turn` beside a tool call is a turn a caller would record as finished.
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[tokio::test]
async fn an_unfinished_stop_reason_is_carried_under_its_own_name() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[4] = message_delta(json!("refusal"), json!({"output_tokens":8}));
    let outcome = decode(&events).await.0.expect("decoded");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "refusal".to_owned()
        }
    );
}

#[tokio::test]
async fn a_provider_error_event_refuses_without_copying_upstream_text() {
    let events = vec![
        message_start(json!({"input_tokens":11,"output_tokens":1})),
        json!({"type":"error","error":{"type":"overloaded_error",
            "message":"Overloaded: internal trace 0xdeadbeef for tenant acme"}}),
    ];
    let (outcome, _) = decode(&events).await;
    let error = outcome.expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(
        !error.message.contains("acme") && !error.message.contains("0xdeadbeef"),
        "upstream text leaked: {}",
        error.message
    );
    assert_eq!(
        error
            .observation
            .expect("snapshot retained")
            .usage
            .expect("usage")
            .output_tokens,
        Some(1)
    );
}

#[tokio::test]
async fn streamed_tool_arguments_are_parsed_once_and_bounded() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{\"query\":"}});
    // Half an argument object never reaches a caller as a whole one.
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);

    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta",
            "partial_json":format!("{{\"query\":\"{}\"}}", "x".repeat(llm_core::MAX_TOOL_ARGUMENT_BYTES))}});
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::TooLarge);
}

#[tokio::test]
async fn a_tool_call_the_request_never_published_is_refused() {
    let mut events = text_turn(
        json!({"input_tokens":11,"output_tokens":1}),
        json!({"output_tokens":8}),
    );
    events[1] = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"unpublished","input":{}}});
    events[2] = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":"{}"}});
    events[4] = message_delta(json!("tool_use"), json!({"output_tokens":8}));
    let (outcome, _) = decode(&events).await;
    assert_eq!(outcome.expect_err("refused").code, ErrorCode::Protocol);
}

#[test]
fn one_complete_message_decodes_through_the_same_codec() {
    let message = json!({
        "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
        "content":[{"type":"text","text":"Hello"}],
        "stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":11,"cache_read_input_tokens":4,
            "cache_creation_input_tokens":6,"output_tokens":8}});
    let outcome = decode_message(&message, &request(), binding().provenance()).expect("decoded");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
    assert!(outcome.observation.final_usage);
    assert_eq!(
        outcome.observation.usage.expect("usage").input_tokens,
        Some(21)
    );
}

#[test]
fn a_complete_message_field_outside_the_declared_subset_is_refused() {
    let mut message = json!({
        "id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201",
        "content":[{"type":"text","text":"Hello"}],"stop_reason":"end_turn",
        "usage":{"input_tokens":11,"output_tokens":8},
        "container":{"id":"container_1","expires_at":"2026-01-01T00:00:00Z"}});
    assert_eq!(
        decode_message(&message, &request(), binding().provenance())
            .expect_err("refused")
            .code,
        ErrorCode::Unsupported
    );
    // A failure still retains what the route did report.
    message["usage"] = json!({"input_tokens":11,"output_tokens":8});
    let error = decode_message(
        &json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[{"type":"image","source":{}}],
            "stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":8}}),
        &request(),
        binding().provenance(),
    )
    .expect_err("refused");
    assert_eq!(
        error
            .observation
            .expect("snapshot retained")
            .usage
            .expect("usage")
            .output_tokens,
        Some(8)
    );
}

/// `src/decode.rs:320`: content blocks are bounded **while the stream is still arriving**, not
/// after it ends.
///
/// Without that bound the only thing that refuses an overrun is the neutral outcome's own late
/// check in `TurnOutcome::validate_for`, which runs once the stream is finished — so a stream that
/// never finishes is never checked at all, and a caller is streamed unbounded content first. This
/// stream therefore carries no terminal event: with the in-flight bound it is refused as too large
/// at the block that crosses it, and the caller sees exactly the bound and no more; without it the
/// same bytes are refused for the unrelated reason that they ran out.
#[tokio::test]
async fn content_blocks_are_bounded_while_the_stream_is_still_arriving() {
    let mut events = vec![message_start(json!({"input_tokens":11,"output_tokens":1}))];
    for index in 0..=u64::try_from(MAX_ITEMS).expect("a representable bound") {
        events.push(json!({"type":"content_block_start","index":index,
            "content_block":{"type":"text","text":""}}));
        events.push(json!({"type":"content_block_delta","index":index,
            "delta":{"type":"text_delta","text":"x"}}));
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    let mut sink = VecSink::new(MAX_ITEMS + 8, 4 * 1024 * 1024);
    let error = decode_stream(
        &sse(&events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await
    .expect_err("a stream whose content exceeds its bound");
    assert_eq!(
        error.code,
        ErrorCode::TooLarge,
        "the overrun was not refused while the stream was arriving; it was carried to the end",
    );
    assert_eq!(
        sink.events().len(),
        MAX_ITEMS,
        "content past the bound reached the caller before anything refused it",
    );
}

/// A field this subset accepts and never consumes still arrives in the shape the producer
/// documents, or it is refused.
///
/// Accepting a name is not the same as ignoring its value: a value in a shape the producer does
/// not document is evidence that the route is not the one this projection was written against,
/// and this codec refuses what it cannot account for rather than passing over it. The class is
/// every name on an accept list that nothing in the codec reads, and it has exactly four members:
/// `service_tier` and `inference_geo` on the usage object, and `stop_sequence` on a message and on
/// a message delta.
#[tokio::test]
async fn an_accepted_field_this_subset_never_consumes_keeps_its_documented_shape() {
    // service_tier, on the usage object of a message start.
    let mut start = message_start(json!({"input_tokens":11,"output_tokens":1}));
    start["message"]["usage"]["service_tier"] = json!(3);
    // inference_geo, on the usage object of a message delta.
    let mut delta = message_delta(json!("end_turn"), json!({"output_tokens":8}));
    delta["usage"]["inference_geo"] = json!(["eu-west"]);
    // stop_sequence, on the message a stream starts with.
    let mut sequence_on_message = message_start(json!({"input_tokens":11,"output_tokens":1}));
    sequence_on_message["message"]["stop_sequence"] = json!(5);
    // stop_sequence, on a message delta.
    let mut sequence_on_delta = message_delta(json!("end_turn"), json!({"output_tokens":8}));
    sequence_on_delta["delta"]["stop_sequence"] = json!({"literal": "END"});
    let good_start = message_start(json!({"input_tokens":11,"output_tokens":1}));
    let good_delta = message_delta(json!("end_turn"), json!({"output_tokens":8}));
    for (name, events) in [
        ("service_tier", vec![start, good_delta.clone()]),
        ("inference_geo", vec![good_start.clone(), delta]),
        (
            "stop_sequence on a message",
            vec![sequence_on_message, good_delta],
        ),
        (
            "stop_sequence on a delta",
            vec![good_start, sequence_on_delta],
        ),
    ] {
        let mut events = events;
        events.push(json!({"type":"message_stop"}));
        let (outcome, _) = decode(&events).await;
        let error = outcome
            .err()
            .unwrap_or_else(|| panic!("{name} was accepted in a shape the producer never sends"));
        assert_eq!(error.code, ErrorCode::Protocol, "{name}");
    }
}

/// The other half of the rule above: the documented shape is accepted, and so is the explicit
/// null the producer writes when the field does not apply. A shape check that refuses what the
/// route really sends is worse than none.
#[tokio::test]
async fn an_accepted_field_in_its_documented_shape_is_carried_without_complaint() {
    let mut start = message_start(json!({"input_tokens":11,"output_tokens":1,
        "service_tier":"standard","inference_geo":"eu-west"}));
    start["message"]["stop_sequence"] = json!("END");
    let mut delta = message_delta(json!("stop_sequence"), json!({"output_tokens":8}));
    delta["delta"]["stop_sequence"] = json!("END");
    let events = vec![start, delta, json!({"type":"message_stop"})];
    let (outcome, _) = decode(&events).await;
    let outcome = outcome.expect("a message reporting its documented metadata");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "stop_sequence".to_owned()
        }
    );
}

/// A value the producer could put in a field; a refusal must never carry it.
const UNDECLARED_VALUE: &str = "value-that-no-diagnostic-may-carry";

fn outside_the_subset(path: &str) -> String {
    format!("Messages field is outside the declared subset: {path}")
}

/// Adds `undeclared` to the object at `pointer` inside `event`.
fn with_undeclared(mut event: Value, pointer: &str) -> Value {
    event
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("{pointer} is an object"))
        .insert("undeclared".to_owned(), json!(UNDECLARED_VALUE));
    event
}

/// A live stream refused with "field is outside the declared subset" and nothing else, so the
/// next run could not say which field to read the producer's documentation for. Every object a
/// stream carries names itself in that refusal, by path from the event, with the field's name and
/// never its value.
#[tokio::test]
async fn a_streamed_field_outside_the_declared_subset_is_refused_by_its_path_and_never_its_value() {
    let usage = json!({"input_tokens":11,"output_tokens":1,
        "cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0},
        "output_tokens_details":{"thinking_tokens":0},
        "server_tool_use":{"web_search_requests":0}});
    // (event position in `text_turn`, pointer inside that event, path the refusal names)
    let cases = [
        (0, "", "message_start.undeclared"),
        (0, "/message", "message_start.message.undeclared"),
        (
            0,
            "/message/usage",
            "message_start.message.usage.undeclared",
        ),
        (
            0,
            "/message/usage/cache_creation",
            "message_start.message.usage.cache_creation.undeclared",
        ),
        (
            0,
            "/message/usage/output_tokens_details",
            "message_start.message.usage.output_tokens_details.undeclared",
        ),
        (
            0,
            "/message/usage/server_tool_use",
            "message_start.message.usage.server_tool_use.undeclared",
        ),
        (1, "", "content_block_start.undeclared"),
        (
            1,
            "/content_block",
            "content_block_start.content_block.undeclared",
        ),
        (2, "", "content_block_delta.undeclared"),
        (2, "/delta", "content_block_delta.delta.undeclared"),
        (3, "", "content_block_stop.undeclared"),
        (4, "", "message_delta.undeclared"),
        (4, "/delta", "message_delta.delta.undeclared"),
        (4, "/usage", "message_delta.usage.undeclared"),
        (5, "", "message_stop.undeclared"),
    ];
    for (at, pointer, path) in cases {
        let mut events = text_turn(usage.clone(), usage.clone());
        events[at] = with_undeclared(events[at].clone(), pointer);
        let (outcome, _) = decode(&events).await;
        let error = outcome.expect_err(path);
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}: {error}");
        assert_eq!(error.message, outside_the_subset(path));
        assert!(!error.message.contains(UNDECLARED_VALUE), "{error}");
    }

    // A keep-alive is an event too.
    let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
    events.insert(1, json!({"type":"ping","undeclared":UNDECLARED_VALUE}));
    let (outcome, _) = decode(&events).await;
    assert_eq!(
        outcome.expect_err("ping").message,
        outside_the_subset("ping.undeclared")
    );

    // A field name is producer text too: one that is not name-shaped is not copied.
    let mut events = text_turn(json!({"input_tokens":11}), json!({"output_tokens":8}));
    events[0]["message"]["usage"]["not a name\n"] = json!(1);
    let (outcome, _) = decode(&events).await;
    assert_eq!(
        outcome.expect_err("unnamed").message,
        outside_the_subset("message_start.message.usage.?")
    );
}

#[test]
fn a_complete_message_field_outside_the_declared_subset_is_refused_by_its_path() {
    let message = || {
        json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[{"type":"text","text":"Hello"}],
            "stop_reason":"end_turn","usage":{"input_tokens":11,"output_tokens":8}})
    };
    for (pointer, path) in [
        ("", "undeclared"),
        ("/usage", "usage.undeclared"),
        ("/content/0", "content[].undeclared"),
    ] {
        let error = decode_message(
            &with_undeclared(message(), pointer),
            &request(),
            binding().provenance(),
        )
        .expect_err(path);
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}: {error}");
        assert_eq!(error.message, outside_the_subset(path));
    }
}
