//! Second adversarial pass over `story:streamed-tool-call-name`. No production source is changed
//! by this file.
//!
//! Driven from the doc comment on `StreamEvent::ToolCallStarted` in `crates/llm-core/src/port.rs`
//! ("A producer emits it once per call") and `docs/messages.md` 65-67.
mod support;

use llm_core::{Cancel, Item, StreamEvent, ToolName, ToolSpec, TurnRequest, VecSink};
use llm_messages::decode_stream;
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

fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

/// Two `tool_use` blocks opened under one `id` are two announcements of one call.
#[tokio::test]
async fn one_tool_use_id_opened_by_two_blocks_is_announced_once() {
    let events = vec![
        json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
            "role":"assistant","model":"example-model-20260201","content":[],
            "stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":11,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"tool_use","id":"call-1","name":"lookup","input":{}}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},
            "usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ];
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        &sse(&events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    let announcements = sink
        .events()
        .iter()
        .filter(|event| matches!(event, StreamEvent::ToolCallStarted { call_id, .. } if call_id.as_str() == "call-1"))
        .count();
    assert!(
        announcements <= 1,
        "call-1 announced {announcements} times, outcome {:?}: {:?}",
        outcome
            .as_ref()
            .map(|_| "accepted")
            .map_err(|error| error.code),
        sink.events()
    );
}
