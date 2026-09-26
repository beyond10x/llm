//! Second adversarial pass over `story:streamed-tool-call-name`. No production source is changed
//! by this file.
//!
//! Driven from two documents the unit wrote: `docs/chat.md` 153-162 (`IngressStream::close`
//! "refuses an outcome that contradicts what the client was already sent: an announced call the
//! outcome does not carry, another name, or other arguments") and the doc comment on
//! `StreamEvent::ToolCallStarted` in `crates/llm-core/src/port.rs` ("A producer emits it once per
//! call").
mod common;

use common::binding;
use llm_chat::{IngressStream, project_response_bytes};
use llm_core::{
    CallId, ErrorCode, Id, Item, StopReason, StreamEvent, ToolCall, ToolName, TurnObservation,
    TurnOutcome,
};
use serde_json::{Value, json};

fn call(id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(id).expect("fixture call"),
        name: ToolName::new(name).expect("fixture tool"),
        arguments,
    })
}

fn outcome(stop_reason: StopReason, items: Vec<Item>) -> TurnOutcome {
    let mut observation = TurnObservation::new(binding().provenance().clone());
    observation.upstream_model = Some(Id::new("fixture-model").expect("fixture model"));
    observation.final_usage = true;
    TurnOutcome {
        stop_reason,
        items,
        observation,
    }
}

fn started(id: &str, name: &str) -> StreamEvent {
    StreamEvent::ToolCallStarted {
        call_id: CallId::new(id).expect("fixture call"),
        name: ToolName::new(name).expect("fixture tool"),
    }
}

fn arguments(id: &str, delta: &str) -> StreamEvent {
    StreamEvent::ToolArgumentsDelta {
        call_id: CallId::new(id).expect("fixture call"),
        delta: delta.to_owned(),
    }
}

/// "another name": the client assembled `call-1` as `lookup`; the outcome calls it `clock`.
/// Only the arguments half of the documented refusal has a case in `tests/ingress.rs`
/// (`streamed_arguments_that_contradict_the_outcome_are_refused`); this is the name half.
#[test]
fn close_refuses_an_outcome_that_renames_an_announced_call() {
    let mut stream = IngressStream::new("chatcmpl-adv2-1", 1_772_000_800);
    stream.chunk(&started("call-1", "lookup"));
    stream.chunk(&arguments("call-1", "{}"));
    let result = stream.close(
        &outcome(
            StopReason::ToolCalls,
            vec![call("call-1", "clock", json!({}))],
        ),
        false,
    );
    assert_eq!(
        result.as_ref().map_err(|error| error.code),
        Err(ErrorCode::Protocol),
        "the client was sent call-1 as lookup and the outcome names it clock: {result:?}"
    );
}

/// "an announced call the outcome does not carry": the client assembled `call-1`; the outcome
/// carries no call at all.
#[test]
fn close_refuses_an_outcome_that_leaves_an_announced_call_out() {
    let mut stream = IngressStream::new("chatcmpl-adv2-2", 1_772_000_800);
    stream.chunk(&started("call-1", "lookup"));
    stream.chunk(&arguments("call-1", "{}"));
    let result = stream.close(
        &outcome(
            StopReason::ToolCalls,
            vec![call("call-2", "clock", json!({}))],
        ),
        false,
    );
    assert_eq!(
        result.as_ref().map_err(|error| error.code),
        Err(ErrorCode::Protocol),
        "the client was sent call-1 and the outcome does not carry it: {result:?}"
    );
}

/// `port.rs`: "A producer emits it once per call". Two wire indices opened under one call
/// identifier are two announcements of one call from the Chat projection.
#[test]
fn a_call_identifier_opened_at_two_indices_is_announced_once() {
    let bytes = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (events, _) = project_response_bytes(bytes.as_bytes(), binding().provenance());
    let announcements = events
        .iter()
        .filter(|event| matches!(event, StreamEvent::ToolCallStarted { call_id, .. } if call_id.as_str() == "call-1"))
        .count();
    assert!(
        announcements <= 1,
        "call-1 was announced {announcements} times: {events:?}"
    );
}
