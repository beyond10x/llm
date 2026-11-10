//! Adversary cases for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! The claim under attack is `StreamEvent::MessageBoundary`'s documentation
//! (`crates/llm-core/src/port.rs`): "the next `TextDelta` begins a new message and never
//! continues one shown before it", so that a re-emitting consumer "can place each shown text in
//! the `Item::AssistantText` it belongs to". The Responses decoder emits the boundary whenever the
//! item changes, and pairs deltas to items by `item_id` wherever they arrive, so deltas of one
//! message that another message's deltas interrupt come back as a third segment continuing the
//! first message.

use llm_core::{Id, Item, Protocol, Provenance, StreamEvent};
use llm_responses::{Binding, decode_stream};
use serde_json::{Value, json};

const UPSTREAM_MODEL: &str = "example/Small-Model";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-endpoint"),
            model: id("small"),
            binding_revision: id("rev-1"),
        },
        id(UPSTREAM_MODEL),
    )
}

fn delta(item_id: &str, output_index: u64, text: &str) -> Value {
    json!({"type": "response.output_text.delta", "item_id": item_id,
        "output_index": output_index, "content_index": 0, "delta": text})
}

fn message(id: &str, text: &str) -> Value {
    json!({"type": "message", "id": id, "role": "assistant",
        "content": [{"type": "output_text", "text": text}]})
}

/// The shown text between boundaries, in order: what a consumer following the documented rule
/// takes for the turn's messages.
fn segments(events: &[StreamEvent]) -> Vec<String> {
    let mut segments = vec![String::new()];
    for event in events {
        match event {
            StreamEvent::MessageBoundary {} => segments.push(String::new()),
            StreamEvent::TextDelta { text } => segments.last_mut().unwrap().push_str(text),
            _ => {}
        }
    }
    segments
}

#[test]
fn text_after_a_boundary_never_continues_a_message_shown_before_it() {
    let payloads = [
        json!({"type": "response.created",
            "response": {"id": "resp_1", "status": "in_progress", "output": []}}),
        delta("msg_a", 0, "Al"),
        delta("msg_b", 1, "Beta."),
        delta("msg_a", 0, "pha."),
        json!({"type": "response.completed", "response": {
            "id": "resp_1", "status": "completed", "model": UPSTREAM_MODEL,
            "output": [message("msg_a", "Alpha."), message("msg_b", "Beta.")],
            "usage": {"input_tokens": 20, "output_tokens": 9}}}),
    ];
    let decoded = decode_stream(&binding(), &payloads);
    let outcome = decoded.result.expect("the turn decodes");
    let messages: Vec<String> = outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::AssistantText { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(messages, ["Alpha.", "Beta."]);
    assert_eq!(
        segments(&decoded.events),
        messages,
        "each run of text between boundaries is one of the turn's messages: {:?}",
        decoded.events
    );
}
