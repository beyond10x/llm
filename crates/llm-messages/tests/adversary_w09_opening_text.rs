//! Adversarial probes for text a Messages `text` block opens with (`story:text-deltas-are-final`,
//! design decision 2.3): shown once as the block opens, folded into the block, never shown
//! twice, kept on the sink when the turn is then refused, and relayed so the ingress encoder
//! closes on the turn. No provider is contacted.
mod support;

use llm_core::{
    CallId, Cancel, Error, Item, StopReason, StreamEvent, ToolCall, ToolName, ToolSpec,
    TurnOutcome, TurnRequest, VecSink,
};
use llm_messages::{IngressStream, decode_stream};
use serde_json::{Value, json};
use support::binding;

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").expect("tool name"),
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

fn message_start() -> Value {
    json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
        "role":"assistant","model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null,
        "usage":{"input_tokens":11,"output_tokens":1}}})
}

fn text(value: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: value.to_owned(),
    }
}

async fn decode(events: &[Value]) -> (Result<TurnOutcome, Error>, VecSink) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        &sse(events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (outcome, sink)
}

/// Two text blocks, each opening with text, a tool call between them, multi-byte text split at
/// the opening: each opening is shown exactly once, each block carries exactly its shown text,
/// and the relay closes on the turn.
#[tokio::test]
async fn opening_text_of_each_block_is_shown_once_and_relayed() {
    let events = vec![
        message_start(),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":"Let me "}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"check"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"input_json_delta","partial_json":"{\"id\":7}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,
            "content_block":{"type":"text","text":"Gr\u{fc}"}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"text_delta","text":"\u{df}e \u{1f600}"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},
            "usage":{"input_tokens":11,"output_tokens":9}}),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let outcome = outcome.expect("a completed turn");
    assert_eq!(
        outcome.items,
        vec![
            Item::assistant("Let me check"),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("toolu_1").expect("call id"),
                name: ToolName::new("lookup").expect("tool name"),
                arguments: json!({"id":7}),
            }),
            Item::assistant("Gr\u{fc}\u{df}e \u{1f600}"),
        ]
    );
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    let shown: Vec<&StreamEvent> = sink
        .events()
        .iter()
        .filter(|event| matches!(event, StreamEvent::TextDelta { .. }))
        .collect();
    assert_eq!(
        shown,
        vec![
            &text("Let me "),
            &text("check"),
            &text("Gr\u{fc}"),
            &text("\u{df}e \u{1f600}")
        ]
    );
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, StreamEvent::Warning { .. })),
        "{:?}",
        sink.events()
    );
    let mut relay = IngressStream::new("msg_014a", Some("example-model-20260201"))
        .expect("a relay for the route's model");
    for event in sink.events() {
        relay.chunk(event);
    }
    relay
        .close(&outcome)
        .unwrap_or_else(|error| panic!("the relay cannot close on the turn: {error:?}"));
}

/// A cut after a block opened with text: the turn is refused and the caller keeps what it was
/// shown, the opening text once.
#[tokio::test]
async fn a_cut_after_opening_text_keeps_it_shown_once() {
    let events = vec![
        message_start(),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":"Hel"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"lo"}}),
    ];
    let (outcome, sink) = decode(&events).await;
    outcome.expect_err("a stream without its terminal event is refused");
    assert_eq!(sink.events(), &[text("Hel"), text("lo")]);
}
