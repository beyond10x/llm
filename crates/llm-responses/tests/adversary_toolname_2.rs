//! Second adversarial pass over `story:streamed-tool-call-name`. No production source is changed
//! by this file.
//!
//! Driven from the doc comment on `StreamEvent::ToolCallStarted` in `crates/llm-core/src/port.rs`
//! ("A producer emits it once per call, before any `ToolArgumentsDelta` for the same `call_id`")
//! and the `docs/responses.md` row for `response.output_item.added`.

use llm_core::{Id, Protocol, Provenance, StreamEvent};
use llm_responses::{Binding, decode_stream};
use serde_json::{Value, json};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-vllm"),
            model: id("small"),
            binding_revision: id("rev-1"),
        },
        id("example/Small-Model"),
    )
}

fn announcements_of(events: &[StreamEvent], call: &str) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, StreamEvent::ToolCallStarted { call_id, .. } if call_id.as_str() == call))
        .count()
}

fn added(item_id: &str, call_id: &str, name: &str) -> Value {
    json!({"type": "response.output_item.added", "item": {
        "type": "function_call", "id": item_id, "call_id": call_id, "name": name, "arguments": ""}})
}

/// The same opening item delivered twice is one call. `stream.rs` `remember_call` guards this
/// with `calls.contains_key(item_id)`; nothing in the suite sends a repeated opening item.
#[test]
fn a_repeated_opening_item_is_announced_once() {
    let payloads = vec![
        added("fc_1", "call-1", "file_read"),
        added("fc_1", "call-1", "file_read"),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-1", "name": "file_read", "arguments": "{}"}]}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert_eq!(
        announcements_of(&decoding.events, "call-1"),
        1,
        "{:?}",
        decoding.events
    );
    decoding.result.expect("terminal");
}

/// Two opening items under one `call_id` are two announcements of one call, and the arguments
/// of both items then stream under that one identifier.
#[test]
fn one_call_identifier_opened_by_two_items_is_announced_once() {
    let payloads = vec![
        added("fc_1", "call-1", "file_read"),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        added("fc_2", "call-1", "file_read"),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_2", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-1", "name": "file_read", "arguments": "{}"}]}}),
    ];
    let decoding = decode_stream(&binding(), &payloads);
    assert!(
        announcements_of(&decoding.events, "call-1") <= 1,
        "call-1 announced {} times, result {:?}: {:?}",
        announcements_of(&decoding.events, "call-1"),
        decoding.result.as_ref().map(|_| "accepted"),
        decoding.events
    );
}
