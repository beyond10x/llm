//! Adversarial pass over `story:streamed-tool-call-name`. No production source is changed by
//! this file.
//!
//! `crates/llm-core/src/port.rs` documents `ToolCallStarted` as the call's name, announced once,
//! "and a consumer re-emitting the stream cannot name the call without it". The Chat projection
//! refuses a fragment that renames an announced call (`chat tool call fragment renames a call
//! already announced`). These cases ask the Responses projection the same question.

use llm_core::{Id, Protocol, Provenance, StreamEvent};
use llm_responses::{Binding, decode_stream};
use serde_json::json;

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

/// Every announcement the caller received names a call the outcome carries under that same
/// name, or the decode is refused.
fn assert_announcements_hold(payloads: &[serde_json::Value]) {
    let decoding = decode_stream(&binding(), payloads);
    let Ok(outcome) = &decoding.result else {
        return;
    };
    for event in &decoding.events {
        if let StreamEvent::ToolCallStarted { call_id, name } = event {
            assert!(
                outcome
                    .tool_calls()
                    .any(|call| &call.call_id == call_id && &call.name == name),
                "announced {call_id}:{name}, outcome carries {:?}",
                outcome
                    .tool_calls()
                    .map(|call| format!("{}:{}", call.call_id, call.name))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn a_finished_call_that_renames_its_announcement_is_not_silently_accepted() {
    assert_announcements_hold(&[
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-1", "name": "file_read"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-1", "name": "clock", "arguments": "{}"}]}}),
    ]);
}

#[test]
fn a_finished_call_under_another_identifier_than_its_announcement_is_not_silently_accepted() {
    assert_announcements_hold(&[
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc_1", "call_id": "call-1", "name": "file_read"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "function_call", "call_id": "call-2", "name": "file_read", "arguments": "{}"}]}}),
    ]);
}
