//! Adversarial pass over `story:streamed-tool-call-name`, driven from `docs/chat.md` 153-159
//! and the doc comments on `IngressStream`. No production source is changed by this file.
mod common;

use common::binding;
use llm_chat::{IngressStream, project_response_bytes};
use llm_core::{
    CallId, Id, Item, StopReason, StreamEvent, ToolCall, ToolName, TurnObservation, TurnOutcome,
};
use serde_json::{Value, json};

fn call(id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(id).expect("fixture call"),
        name: ToolName::new(name).expect("fixture tool"),
        arguments,
    })
}

fn outcome(items: Vec<Item>) -> TurnOutcome {
    let mut observation = TurnObservation::new(binding().provenance().clone());
    observation.upstream_model = Some(Id::new("fixture-model").expect("fixture model"));
    observation.final_usage = true;
    TurnOutcome {
        stop_reason: StopReason::ToolCalls,
        items,
        observation,
    }
}

/// `docs/chat.md` 155-159: `close` "refuses an outcome that contradicts what the client was
/// already sent". The Chat response projection in this same crate accepts a whitespace-only
/// argument text as `{}` (`incoming.rs` `parse_arguments` trims) and relays that whitespace as
/// a delta. Fed straight into the Chat re-emitter, the outcome the projection produced is
/// refused as contradicting the stream the projection produced.
#[test]
fn a_chat_stream_relayed_through_the_chat_reemitter_closes_when_its_arguments_are_blank() {
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"clock\",\"arguments\":\" \"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (events, projected) = project_response_bytes(bytes.as_bytes(), binding().provenance());
    let projected = projected.expect("the projection accepts blank arguments as {}");
    assert!(
        projected
            .tool_calls()
            .any(|call| call.arguments == json!({}))
    );

    let mut stream = IngressStream::new("chatcmpl-adv-1", 1_772_000_000);
    for event in &events {
        stream.chunk(event);
    }
    let closed = stream.close(&projected, false);
    assert!(
        closed.is_ok(),
        "the re-emitter refused the projection's own outcome: {closed:?}"
    );
}

/// `ingress.rs` `IngressStream::announced`: "a call's position here is its wire index", and an
/// unannounced call is emitted "complete at close" under the next index. With one announced
/// call whose arguments never streamed, followed by one call the stream never announced, the
/// unannounced call must take wire index 1 — the next one — not skip it.
#[test]
fn a_call_completed_at_close_and_an_unannounced_call_take_consecutive_wire_indices() {
    let mut stream = IngressStream::new("chatcmpl-adv-2", 1_772_000_000);
    assert!(
        stream
            .chunk(&StreamEvent::ToolCallStarted {
                call_id: CallId::new("call-1").expect("fixture call"),
                name: ToolName::new("clock").expect("fixture tool"),
            })
            .is_some()
    );
    let closing = stream
        .close(
            &outcome(vec![
                call("call-1", "clock", json!({})),
                call("call-2", "lookup", json!({"city": "Oslo"})),
            ]),
            false,
        )
        .expect("closed");
    let indices: Vec<&Value> = closing[0]["choices"][0]["delta"]["tool_calls"]
        .as_array()
        .expect("the calls completed at close")
        .iter()
        .map(|call| &call["index"])
        .collect();
    assert_eq!(indices, [&json!(0), &json!(1)]);
}
