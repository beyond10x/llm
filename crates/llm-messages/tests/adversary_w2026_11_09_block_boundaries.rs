//! Adversary cases for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! The claim under attack is `StreamEvent`'s own documentation (`crates/llm-core/src/port.rs`):
//! a `MessageBoundary` "carries nothing" and is "not model output", and an `OpaqueItem` is
//! something "any other consumer ignores". `VecSink`, the bounded sink this crate's own tests and
//! the embedded examples hand a turn, does not ignore either: it stores both and charges both
//! against the caller's event and byte bounds. A turn that fit its sink before this change is now
//! refused `too-large` after output, so the refusal is final.
mod support;

use llm_core::{Cancel, Item, Provenance, StreamEvent, TurnRequest, VecSink};
use llm_messages::{decode_request, decode_stream};
use serde_json::{Value, json};
use support::binding;

const ID: &str = "msg_adversary_1";
const MODEL: &str = "example-model-20260201";

fn target() -> Provenance {
    binding().provenance().clone()
}

fn request() -> TurnRequest {
    let body = json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role":"user","content":"Summarise the log"}],
        "tools": [
            {"name":"lookup","description":"Look up a record","input_schema":{"type":"object"}},
        ],
        "stream": true,
    });
    decode_request(body.to_string().as_bytes(), &target())
        .expect("a Messages request")
        .request
}

fn sse(payloads: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for payload in payloads {
        let name = payload["type"].as_str().expect("a typed payload");
        bytes.extend_from_slice(format!("event: {name}\ndata: {payload}\n\n").as_bytes());
    }
    bytes
}

fn start() -> Value {
    json!({"type":"message_start","message":{"id":ID,"type":"message","role":"assistant",
        "model":MODEL,"content":[]}})
}

fn end() -> [Value; 2] {
    [
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
            "usage":{"output_tokens":9}}),
        json!({"type":"message_stop"}),
    ]
}

/// The bound `crates/llm-core/examples/embedded.rs` gives its sink: 16 events, 4 KiB.
const EXAMPLE_EVENTS: usize = 16;
const EXAMPLE_BYTES: usize = 4096;

/// A redacted thinking block shows the caller nothing, and before this change emitted nothing.
/// Its encrypted data is now copied into an `OpaqueItem` and charged against the caller's sink:
/// 8 KiB of it exceeds the example's 4 KiB budget and the turn fails after the block started,
/// although the only text the caller is shown is five bytes.
#[tokio::test]
async fn redacted_thinking_does_not_spend_the_callers_sink_budget() {
    let data = "A".repeat(8 * 1024);
    let mut upstream = vec![
        start(),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"redacted_thinking","data":data}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Done."}}),
        json!({"type":"content_block_stop","index":1}),
    ];
    upstream.extend(end());
    let mut sink = VecSink::new(EXAMPLE_EVENTS, EXAMPLE_BYTES);
    let decoded = decode_stream(
        &sse(&upstream),
        &request(),
        &target(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    let outcome = decoded.expect(
        "a turn showing five bytes of text fits a 4 KiB sink: redacted thinking is not shown",
    );
    assert_eq!(outcome.items.len(), 2);
    assert!(matches!(outcome.items[0], Item::Opaque { .. }));
    assert_eq!(sink.text(), "Done.");
}

/// A message boundary "carries nothing" and is "not model output", yet takes one of the caller's
/// event slots. Text, a call and text show exactly four events; a sink bounded at four held this
/// turn before the boundary existed and now refuses its last text.
#[tokio::test]
async fn a_message_boundary_takes_no_event_slot_of_the_callers_sink() {
    let mut upstream = vec![
        start(),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"First."}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,
            "content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"text_delta","text":"Second."}}),
        json!({"type":"content_block_stop","index":2}),
    ];
    // Correction round 1: a turn with a call stops for `tool_use`; `end_turn` was refused as
    // contradicting its tool obligations whatever the sink held.
    upstream.extend([
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},
            "usage":{"output_tokens":9}}),
        json!({"type":"message_stop"}),
    ]);
    let mut sink = VecSink::new(4, 64 * 1024);
    let decoded = decode_stream(
        &sse(&upstream),
        &request(),
        &target(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    let shown: Vec<&StreamEvent> = sink
        .events()
        .iter()
        .filter(|event| !matches!(event, StreamEvent::MessageBoundary {}))
        .collect();
    decoded.expect("four shown events fit a sink of four: a boundary is not output");
    assert_eq!(shown.len(), 4);
    assert_eq!(sink.text(), "First.Second.");
}
