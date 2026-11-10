//! Adversary cases for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! The claim under attack is `StreamEvent::MessageBoundary`'s documentation
//! (`crates/llm-core/src/port.rs`). It first promised that the text after a boundary "never
//! continues one shown before it"; the Responses decoder emits the boundary whenever the item
//! changes and pairs deltas to items by `item_id` wherever they arrive, so deltas of one message
//! that another message's deltas interrupt come back as a third segment continuing the first.
//!
//! Decided (correction round 1, row 3): no server is known to interleave, so the promise is
//! narrowed to what holds. Messages streamed one after another are separated by exactly one
//! boundary; interleaved deltas of two items give one boundary per change.

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

fn text(delta: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: delta.to_owned(),
    }
}

fn messages(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::AssistantText { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn created() -> Value {
    json!({"type": "response.created",
        "response": {"id": "resp_1", "status": "in_progress", "output": []}})
}

fn completed() -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "status": "completed", "model": UPSTREAM_MODEL,
        "output": [message("msg_a", "Alpha."), message("msg_b", "Beta.")],
        "usage": {"input_tokens": 20, "output_tokens": 9}}})
}

#[test]
fn messages_streamed_one_after_another_are_separated_by_exactly_one_boundary() {
    let payloads = [
        created(),
        delta("msg_a", 0, "Al"),
        delta("msg_a", 0, "pha."),
        delta("msg_b", 1, "Beta."),
        completed(),
    ];
    let decoded = decode_stream(&binding(), &payloads);
    let outcome = decoded.result.expect("the turn decodes");
    assert_eq!(messages(&outcome.items), ["Alpha.", "Beta."]);
    assert_eq!(
        segments(&decoded.events),
        messages(&outcome.items),
        "each run of text between boundaries is one of the turn's messages: {:?}",
        decoded.events
    );
}

#[test]
fn interleaved_deltas_of_two_messages_give_one_boundary_per_change() {
    let payloads = [
        created(),
        delta("msg_a", 0, "Al"),
        delta("msg_b", 1, "Beta."),
        delta("msg_a", 0, "pha."),
        completed(),
    ];
    let decoded = decode_stream(&binding(), &payloads);
    let outcome = decoded.result.expect("the turn decodes");
    assert_eq!(messages(&outcome.items), ["Alpha.", "Beta."]);
    assert_eq!(
        decoded.events,
        [
            text("Al"),
            StreamEvent::MessageBoundary {},
            text("Beta."),
            StreamEvent::MessageBoundary {},
            text("pha."),
        ],
        "decided in correction round 1, row 3: interleaved deltas of two items give one \
         boundary per change of item, and the text after the second continues the first message"
    );
}
